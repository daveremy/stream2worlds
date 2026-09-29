//! Operator-authored viewer presentation for a world: title, palette, and typefaces.
//!
//! Stored append-only (latest row per world wins), mirroring the log's other tables. Unlike
//! [`crate::WorldManifest`], which is create-once identity, presentation may be revised any
//! number of times — and a future "discovered presentation" (a follow-up issue) needs to tell a
//! proposed record from an accepted one, hence the `origin` column from the start.
use crate::{LogError, ReadOnlySqliteEventLog, SqliteEventLog, map_sqlite};
use rusqlite::{Connection, OptionalExtension, params};
use s2w_model::Timestamp;
use serde::{Deserialize, Serialize};

const MAX_TITLE_BYTES: usize = 120;
const MAX_TAGLINE_BYTES: usize = 200;
const MAX_DESCRIPTION_BYTES: usize = 8192;
const MAX_STYLESHEET_BYTES: usize = 16 * 1024;

/// Where a presentation record came from. Only `Operator` is written today; `Discovered` is
/// reserved for a follow-up issue so the schema does not need a second migration to add it.
pub(crate) const ORIGIN_OPERATOR: &str = "operator";

/// A six-token color palette applied as CSS custom properties. Intentionally non-optional
/// fields: a partial palette looks broken in the viewer, so a presentation either supplies all
/// six tokens or none (`WorldPresentation::palette_light`/`palette_dark` stay `Option`-wrapped
/// at the outer level instead).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    /// Page/canvas background, hex (`#rgb`, `#rgba`, `#rrggbb`, or `#rrggbbaa`).
    #[serde(default)]
    pub ground: String,
    /// Primary text and default canvas ink color, hex.
    #[serde(default)]
    pub ink: String,
    /// Primary accent color, hex.
    #[serde(default)]
    pub accent: String,
    /// Success/positive state color, hex.
    #[serde(default)]
    pub success: String,
    /// Warning state color, hex.
    #[serde(default)]
    pub warning: String,
    /// Danger/error state color, hex.
    #[serde(default)]
    pub danger: String,
}

/// Font stack names for the three roles the viewer distinguishes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Typefaces {
    /// Heading/display font stack.
    #[serde(default)]
    pub display: String,
    /// Body text font stack.
    #[serde(default)]
    pub body: String,
    /// Monospace font stack.
    #[serde(default)]
    pub mono: String,
}

/// The load-path type: tolerant of an older or partially-written row (every field defaults on
/// missing/unknown-shaped input) so an older binary reading a newer row degrades gracefully
/// instead of erroring. Validation still runs on the write path (see [`WorldPresentationInput`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldPresentation {
    /// Overrides the viewer's `document.title` and any name/world-derived fallback.
    #[serde(default)]
    pub title: Option<String>,
    /// Short one-line summary shown in the viewer header.
    #[serde(default)]
    pub tagline: Option<String>,
    /// Markdown text, rendered plain (`textContent`) by the viewer for this PR — no sanitizer
    /// exists in this workspace, and adding one is out of scope here.
    #[serde(default)]
    pub description: Option<String>,
    /// Palette applied when the viewer resolves to a light color scheme.
    #[serde(default)]
    pub palette_light: Option<Palette>,
    /// Palette applied when the viewer resolves to a dark color scheme.
    #[serde(default)]
    pub palette_dark: Option<Palette>,
    /// Font stacks for the viewer's display/body/mono roles.
    #[serde(default)]
    pub typefaces: Option<Typefaces>,
    /// Optional per-world CSS, the escape hatch beyond palette and typefaces. Operator-supplied
    /// text: size-capped and free of anything that loads a resource or breaks out of a style
    /// element (see `validate_stylesheet`). The viewer injects it on this world's page only,
    /// never on the home page or another world's page.
    #[serde(default)]
    pub stylesheet: Option<String>,
}

/// The write-path (CLI `set`) input type: rejects an unrecognized key loudly instead of
/// silently dropping a typo'd field. Kept as a separate type from [`WorldPresentation`] because
/// `deny_unknown_fields` and load-path tolerance cannot both apply to one struct.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldPresentationInput {
    /// See [`WorldPresentation::title`].
    #[serde(default)]
    pub title: Option<String>,
    /// See [`WorldPresentation::tagline`].
    #[serde(default)]
    pub tagline: Option<String>,
    /// See [`WorldPresentation::description`].
    #[serde(default)]
    pub description: Option<String>,
    /// See [`WorldPresentation::palette_light`].
    #[serde(default)]
    pub palette_light: Option<Palette>,
    /// See [`WorldPresentation::palette_dark`].
    #[serde(default)]
    pub palette_dark: Option<Palette>,
    /// See [`WorldPresentation::typefaces`].
    #[serde(default)]
    pub typefaces: Option<Typefaces>,
    /// See [`WorldPresentation::stylesheet`].
    #[serde(default)]
    pub stylesheet: Option<String>,
}

impl From<WorldPresentationInput> for WorldPresentation {
    fn from(input: WorldPresentationInput) -> Self {
        Self {
            title: input.title,
            tagline: input.tagline,
            description: input.description,
            palette_light: input.palette_light,
            palette_dark: input.palette_dark,
            typefaces: input.typefaces,
            stylesheet: input.stylesheet,
        }
    }
}

fn is_valid_hex_color(value: &str) -> bool {
    let Some(digits) = value.strip_prefix('#') else {
        return false;
    };
    matches!(digits.len(), 3 | 4 | 6 | 8) && digits.chars().all(|c| c.is_ascii_hexdigit())
}

/// A typeface entry: letters, digits, spaces, commas, periods, and hyphens. Quote characters
/// are rejected outright (not merely balance-checked) — a dangling `"Foo` currently fails
/// silently in the browser rather than hitting the CSS `var()` fallback, and quoted font stacks
/// are not a need this PR has evidence for yet.
fn is_valid_typeface(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | ',' | '.' | '-'))
}

/// Rejects stylesheet text that could load a remote or embedded resource, break out of the
/// `<style>` element it is injected into, or hide such a construct behind a CSS escape.
/// A denylist by design: the sheet is operator-supplied and the viewer never runs script from
/// CSS, so the goal is "no network, no escape hatch out of the sheet", not a CSS parser.
/// The denylist runs on the raw text AND on the comment-stripped text: a browser tokenizes
/// strings before comments, so a `/*` inside a string must not let the validator skip real
/// code (`content:"/*"} x{background:url(..)} y{content:"*/"`), while a comment splitting a
/// name is not a valid token and needs no special handling.
fn validate_stylesheet(css: &str) -> Result<(), LogError> {
    let bad = |reason: &str| {
        Err(LogError::InvalidPresentation(format!(
            "stylesheet {reason}"
        )))
    };
    if css.len() > MAX_STYLESHEET_BYTES {
        return bad(&format!("exceeds {MAX_STYLESHEET_BYTES} bytes"));
    }
    if css.contains('\\') {
        return bad("contains a backslash (CSS escapes are not allowed)");
    }
    if css.contains('<') || css.contains('>') {
        return bad("contains '<' or '>' (markup is not allowed)");
    }
    if css
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return bad("contains a control character");
    }
    let mut stripped = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        stripped.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => {
                stripped.push(' ');
                rest = &rest[start + 2 + end + 2..];
            }
            None => return bad("has an unterminated comment"),
        }
    }
    stripped.push_str(rest);
    let lowers = [css.to_ascii_lowercase(), stripped.to_ascii_lowercase()];
    for (needle, what) in [
        ("@import", "@import"),
        ("@namespace", "@namespace"),
        ("@charset", "@charset"),
        ("@font-face", "@font-face"),
        ("url(", "url()"),
        ("url (", "url()"),
        ("src(", "src()"),
        ("image-set(", "image-set()"),
        ("image(", "image()"),
        ("cross-fade(", "cross-fade()"),
        ("element(", "element()"),
        ("expression(", "expression()"),
        ("-moz-binding", "-moz-binding"),
        ("javascript:", "javascript:"),
        ("://", "a remote address"),
    ] {
        if lowers.iter().any(|lower| lower.contains(needle)) {
            return bad(&format!("contains {what}, which is not allowed"));
        }
    }
    Ok(())
}

fn validate_palette(palette: &Palette) -> Result<(), LogError> {
    for (field, value) in [
        ("ground", &palette.ground),
        ("ink", &palette.ink),
        ("accent", &palette.accent),
        ("success", &palette.success),
        ("warning", &palette.warning),
        ("danger", &palette.danger),
    ] {
        if !is_valid_hex_color(value) {
            return Err(LogError::InvalidPresentation(format!(
                "palette.{field} is not a valid hex color: {value:?}"
            )));
        }
    }
    Ok(())
}

impl WorldPresentation {
    /// Validates every present field. Called from [`WorldPresentation::set`]; never re-validated
    /// on the load path, which only ever reads back what `set` already validated (and stays
    /// tolerant of an older row via `#[serde(default)]`).
    ///
    /// # Errors
    /// [`LogError::InvalidPresentation`] naming the first field that fails.
    pub fn validate(&self) -> Result<(), LogError> {
        if let Some(title) = &self.title
            && title.len() > MAX_TITLE_BYTES
        {
            return Err(LogError::InvalidPresentation(format!(
                "title exceeds {MAX_TITLE_BYTES} bytes"
            )));
        }
        if let Some(tagline) = &self.tagline
            && tagline.len() > MAX_TAGLINE_BYTES
        {
            return Err(LogError::InvalidPresentation(format!(
                "tagline exceeds {MAX_TAGLINE_BYTES} bytes"
            )));
        }
        if let Some(description) = &self.description
            && description.len() > MAX_DESCRIPTION_BYTES
        {
            return Err(LogError::InvalidPresentation(format!(
                "description exceeds {MAX_DESCRIPTION_BYTES} bytes"
            )));
        }
        if let Some(palette) = &self.palette_light {
            validate_palette(palette)?;
        }
        if let Some(palette) = &self.palette_dark {
            validate_palette(palette)?;
        }
        if let Some(typefaces) = &self.typefaces {
            for (field, value) in [
                ("display", &typefaces.display),
                ("body", &typefaces.body),
                ("mono", &typefaces.mono),
            ] {
                if !is_valid_typeface(value) {
                    return Err(LogError::InvalidPresentation(format!(
                        "typefaces.{field} is not a valid font stack entry: {value:?}"
                    )));
                }
            }
        }
        if let Some(stylesheet) = &self.stylesheet {
            validate_stylesheet(stylesheet)?;
        }
        Ok(())
    }

    /// Validates and appends a new presentation record for `world`. Refuses a world with no
    /// [`crate::WorldManifest`] row — presentation cannot exist for a world that does not.
    ///
    /// # Errors
    /// [`LogError::InvalidPresentation`] on a validation failure; [`LogError::Corrupt`] if
    /// `world` has no manifest.
    pub fn set(
        log: &mut SqliteEventLog,
        world: &str,
        presentation: &WorldPresentation,
        now: Timestamp,
    ) -> Result<(), LogError> {
        presentation.validate()?;
        if crate::WorldManifest::load(log, world)?.is_none() {
            return Err(LogError::Corrupt(format!(
                "world {world:?} has no manifest; cannot set presentation for a world that does not exist"
            )));
        }
        let data = serde_json::to_string(presentation)
            .map_err(|error| LogError::InvalidPresentation(error.to_string()))?;
        log.connection
            .execute(
                "INSERT INTO world_presentation(world, created_at, origin, data) VALUES (?1,?2,?3,?4)",
                params![world, now.as_millis(), ORIGIN_OPERATOR, data],
            )
            .map_err(map_sqlite)?;
        Ok(())
    }

    /// Loads the latest presentation record for `world`, or `None` if none has ever been set.
    ///
    /// # Errors
    /// [`LogError::Corrupt`] if the stored row cannot be decoded — a corrupt row is a loud
    /// error, never a silent fresh start.
    pub fn load(connection: &Connection, world: &str) -> Result<Option<Self>, LogError> {
        load_from(connection, world)
    }
}

fn load_from(connection: &Connection, world: &str) -> Result<Option<WorldPresentation>, LogError> {
    let data: Option<String> = connection
        .query_row(
            "SELECT data FROM world_presentation WHERE world=?1 ORDER BY seq DESC LIMIT 1",
            [world],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_sqlite)?;
    let Some(data) = data else {
        return Ok(None);
    };
    serde_json::from_str(&data)
        .map(Some)
        .map_err(|error| LogError::Corrupt(format!("undecodable presentation row: {error}")))
}

impl ReadOnlySqliteEventLog {
    /// Loads the latest presentation record for `world` without taking the writer lock.
    pub fn world_presentation(&self, world: &str) -> Result<Option<WorldPresentation>, LogError> {
        load_from(&self.connection, world)
    }
}

pub(crate) fn initialize(connection: &Connection) -> Result<(), LogError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS world_presentation (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                world TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                origin TEXT NOT NULL DEFAULT 'operator',
                data TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS world_presentation_world_seq ON world_presentation(world, seq);
            CREATE TRIGGER IF NOT EXISTS world_presentation_no_UPDATE
            BEFORE UPDATE ON world_presentation BEGIN
                SELECT RAISE(ABORT, 'world_presentation is append-only'); END;
            CREATE TRIGGER IF NOT EXISTS world_presentation_no_DELETE
            BEFORE DELETE ON world_presentation BEGIN
                SELECT RAISE(ABORT, 'world_presentation is append-only'); END;",
        )
        .map_err(map_sqlite)
}

/// Adds the `world_presentation` table to a v3 database and bumps `user_version` to 4. Mirrors
/// [`crate::membership::migrate_v2_to_v3`]'s shape: idempotent `CREATE TABLE IF NOT EXISTS`, run
/// inside the caller's transaction.
pub(crate) fn migrate_v3_to_v4(connection: &mut Connection) -> Result<(), LogError> {
    let tx = connection.transaction().map_err(map_sqlite)?;
    initialize(&tx)?;
    tx.execute_batch("PRAGMA user_version = 4;")
        .map_err(map_sqlite)?;
    tx.commit().map_err(map_sqlite)
}

#[cfg(test)]
mod tests;
