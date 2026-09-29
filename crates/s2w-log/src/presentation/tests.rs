use super::*;
use crate::tests::{TestDirectory, retry_until_unlocked};
use crate::{EventLog, WorldManifest};
use rusqlite::Connection;
use s2w_model::{Cursor, RawEvent, SourceId};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn palette(prefix: &str) -> Palette {
    Palette {
        ground: format!("#{prefix}0"),
        ink: format!("#{prefix}1"),
        accent: format!("#{prefix}2"),
        success: format!("#{prefix}3"),
        warning: format!("#{prefix}4"),
        danger: format!("#{prefix}5"),
    }
}

fn make_world(log: &mut SqliteEventLog, world: &str) -> Result<(), LogError> {
    WorldManifest::create_if_absent(
        log,
        world,
        "test world",
        Timestamp::from_millis(1),
        &[],
        &[],
    )
    .map(|_| ())
}

#[test]
fn round_trips_set_and_load() -> TestResult {
    let dir = TestDirectory::new("presentation-roundtrip")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    make_world(&mut log, "w1")?;
    let presentation = WorldPresentation {
        title: Some("Hello".to_owned()),
        tagline: Some("A tagline".to_owned()),
        description: Some("Some description".to_owned()),
        palette_light: Some(palette("abcde")),
        palette_dark: Some(palette("fedcb")),
        typefaces: Some(Typefaces {
            display: "Inter".to_owned(),
            body: "system-ui".to_owned(),
            mono: "monospace".to_owned(),
        }),
        stylesheet: Some("h1 { font-weight: 300; }".to_owned()),
    };
    WorldPresentation::set(&mut log, "w1", &presentation, Timestamp::from_millis(2))?;
    let loaded = WorldPresentation::load(&log.connection, "w1")?;
    assert_eq!(loaded, Some(presentation));
    Ok(())
}

#[test]
fn load_returns_none_when_never_set() -> TestResult {
    let dir = TestDirectory::new("presentation-absent")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    make_world(&mut log, "w1")?;
    assert_eq!(WorldPresentation::load(&log.connection, "w1")?, None);
    Ok(())
}

#[test]
fn set_refuses_orphan_world() -> TestResult {
    let dir = TestDirectory::new("presentation-orphan")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    let presentation = WorldPresentation::default();
    match WorldPresentation::set(&mut log, "ghost", &presentation, Timestamp::from_millis(1)) {
        Err(LogError::Corrupt(message)) => assert!(message.contains("no manifest")),
        other => panic!("expected Corrupt for an orphan world, got {other:?}"),
    }
    Ok(())
}

#[test]
fn set_appends_latest_row_wins() -> TestResult {
    let dir = TestDirectory::new("presentation-append")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    make_world(&mut log, "w1")?;
    let first = WorldPresentation {
        title: Some("First".to_owned()),
        ..Default::default()
    };
    let second = WorldPresentation {
        title: Some("Second".to_owned()),
        ..Default::default()
    };
    WorldPresentation::set(&mut log, "w1", &first, Timestamp::from_millis(1))?;
    WorldPresentation::set(&mut log, "w1", &second, Timestamp::from_millis(2))?;
    assert_eq!(
        WorldPresentation::load(&log.connection, "w1")?,
        Some(second)
    );
    let count: i64 = log.connection.query_row(
        "SELECT COUNT(*) FROM world_presentation WHERE world='w1'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 2, "set never overwrites, only appends");
    Ok(())
}

#[test]
fn validate_rejects_oversized_title_tagline_description() {
    let mut presentation = WorldPresentation {
        title: Some("x".repeat(121)),
        ..Default::default()
    };
    assert!(presentation.validate().is_err());
    presentation.title = None;
    presentation.tagline = Some("x".repeat(201));
    assert!(presentation.validate().is_err());
    presentation.tagline = None;
    presentation.description = Some("x".repeat(8193));
    assert!(presentation.validate().is_err());
    presentation.description = Some("x".repeat(8192));
    assert!(presentation.validate().is_ok());
}

#[test]
fn validate_hex_color_boundaries() {
    let valid = ["#abc", "#abcd", "#aabbcc", "#aabbccdd"];
    let invalid = ["#abcde", "#aabbc", "abcdef", "#ghi", "#", ""];
    for hex in valid {
        let mut palette = palette("11111");
        palette.ground = hex.to_owned();
        assert!(
            WorldPresentation {
                palette_light: Some(palette),
                ..Default::default()
            }
            .validate()
            .is_ok(),
            "{hex} should be valid"
        );
    }
    for hex in invalid {
        let mut palette = palette("11111");
        palette.ground = hex.to_owned();
        assert!(
            WorldPresentation {
                palette_light: Some(palette),
                ..Default::default()
            }
            .validate()
            .is_err(),
            "{hex} should be invalid"
        );
    }
}

#[test]
fn validate_typeface_charset() {
    let valid = ["Inter", "system-ui", "Times New Roman", "Foo, Bar"];
    let invalid = ["\"Foo", "Foo'", "<script>", ""];
    for value in valid {
        assert!(
            WorldPresentation {
                typefaces: Some(Typefaces {
                    display: value.to_owned(),
                    body: "system-ui".to_owned(),
                    mono: "monospace".to_owned(),
                }),
                ..Default::default()
            }
            .validate()
            .is_ok(),
            "{value:?} should be valid"
        );
    }
    for value in invalid {
        assert!(
            WorldPresentation {
                typefaces: Some(Typefaces {
                    display: value.to_owned(),
                    body: "system-ui".to_owned(),
                    mono: "monospace".to_owned(),
                }),
                ..Default::default()
            }
            .validate()
            .is_err(),
            "{value:?} should be invalid"
        );
    }
}

#[test]
fn strict_input_type_rejects_unknown_fields() {
    let result: Result<WorldPresentationInput, _> =
        serde_json::from_str(r#"{"title":"ok","typo_field":true}"#);
    assert!(result.is_err());
}

#[test]
fn load_path_tolerates_partial_row() -> TestResult {
    let dir = TestDirectory::new("presentation-partial")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    make_world(&mut log, "w1")?;
    // A row written by an older binary before some field existed: only `title` present.
    log.connection.execute(
        "INSERT INTO world_presentation(world, created_at, origin, data) VALUES (?1,?2,?3,?4)",
        rusqlite::params!["w1", 1_i64, ORIGIN_OPERATOR, r#"{"title":"legacy"}"#],
    )?;
    let loaded = WorldPresentation::load(&log.connection, "w1")?;
    assert_eq!(
        loaded,
        Some(WorldPresentation {
            title: Some("legacy".to_owned()),
            ..Default::default()
        })
    );
    Ok(())
}

#[test]
fn corrupt_row_is_a_loud_error_not_a_silent_none() -> TestResult {
    let dir = TestDirectory::new("presentation-corrupt")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    make_world(&mut log, "w1")?;
    log.connection.execute(
        "INSERT INTO world_presentation(world, created_at, origin, data) VALUES (?1,?2,?3,?4)",
        rusqlite::params!["w1", 1_i64, ORIGIN_OPERATOR, "not json"],
    )?;
    match WorldPresentation::load(&log.connection, "w1") {
        Err(LogError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for an undecodable row, got {other:?}"),
    }
    Ok(())
}

#[test]
fn append_only_triggers_refuse_update_and_delete() -> TestResult {
    let dir = TestDirectory::new("presentation-triggers")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    make_world(&mut log, "w1")?;
    WorldPresentation::set(
        &mut log,
        "w1",
        &WorldPresentation::default(),
        Timestamp::from_millis(1),
    )?;
    assert!(
        log.connection
            .execute(
                "UPDATE world_presentation SET data='{}' WHERE world='w1'",
                []
            )
            .is_err()
    );
    assert!(
        log.connection
            .execute("DELETE FROM world_presentation WHERE world='w1'", [])
            .is_err()
    );
    Ok(())
}

fn source() -> Result<SourceId, s2w_model::ModelError> {
    SourceId::new("s")
}

fn event(n: u8) -> Result<RawEvent, s2w_model::ModelError> {
    Ok(RawEvent {
        source: source()?,
        cursor: Cursor::new(vec![n])?,
        received_at: Timestamp::from_millis(i64::from(n)),
        payload: vec![n],
    })
}

/// Opens a v3 fixture DB (schema exactly as `migrate_v2_to_v3` leaves it, no
/// `world_presentation` table) and confirms `open()` migrates it to v4: the new table exists,
/// `user_version` reads 4, and pre-existing data survives untouched.
#[test]
fn migrates_v3_fixture_adding_presentation_table() -> TestResult {
    let dir = TestDirectory::new("presentation-v3-migration")?;
    let conn = Connection::open(dir.path().join(crate::DATABASE_FILE))?;
    conn.execute_batch(
        "CREATE TABLE events(position INTEGER PRIMARY KEY AUTOINCREMENT,source TEXT NOT NULL,cursor BLOB NOT NULL,received_at INTEGER NOT NULL,payload BLOB NOT NULL,content_hash INTEGER NOT NULL,UNIQUE(source,content_hash));
        CREATE TABLE cursors(source TEXT PRIMARY KEY,cursor BLOB NOT NULL,last_position INTEGER REFERENCES events(position));
        CREATE TABLE membership(seq INTEGER PRIMARY KEY AUTOINCREMENT, source TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('added','removed')), effective_from BLOB,
            world_offset INTEGER NOT NULL);
        CREATE TABLE world_manifest(world TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL);
        CREATE TABLE world_manifest_engines(world TEXT NOT NULL, engine TEXT NOT NULL);
        CREATE TABLE world_manifest_policies(world TEXT NOT NULL, policy TEXT NOT NULL);
        PRAGMA user_version=3;",
    )?;
    let raw = event(1)?;
    conn.execute(
        "INSERT INTO events VALUES (1,?1,?2,?3,?4,?5)",
        rusqlite::params![
            raw.source.as_str(),
            raw.cursor.as_bytes(),
            raw.received_at.as_millis(),
            raw.payload,
            crate::content_hash(&raw.payload)
        ],
    )?;
    drop(conn);
    let log = retry_until_unlocked(|| SqliteEventLog::open(dir.path()))?;
    assert_eq!(
        log.connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        4
    );
    assert_eq!(
        log.connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='world_presentation'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        1
    );
    assert_eq!(log.replay(None)?.next().ok_or("missing event")??.event, raw);
    Ok(())
}

fn css_presentation(css: &str) -> WorldPresentation {
    WorldPresentation {
        stylesheet: Some(css.to_owned()),
        ..WorldPresentation::default()
    }
}

#[test]
fn stylesheet_accepts_plain_css_and_round_trips() -> TestResult {
    let dir = TestDirectory::new("presentation-stylesheet")?;
    let mut log = SqliteEventLog::open(dir.path())?;
    make_world(&mut log, "w1")?;
    let css = "html { scroll-behavior: smooth; } /* calm */ body { letter-spacing: .02em; }\n@media (prefers-color-scheme: light) { h1 { color: #123; } }";
    WorldPresentation::set(
        &mut log,
        "w1",
        &css_presentation(css),
        Timestamp::from_millis(2),
    )?;
    let loaded = WorldPresentation::load(&log.connection, "w1")?.ok_or("missing")?;
    assert_eq!(loaded.stylesheet.as_deref(), Some(css));
    Ok(())
}

#[test]
fn stylesheet_rejects_resource_loading_and_escapes() {
    for css in [
        "@import 'x.css';",
        "@IMPORT url(x)",
        "body{background:url(data:image/png;base64,AA)}",
        "body{background:URL (x)}",
        "body{background:image-set('a' 1x)}",
        "@font-face{font-family:x}",
        "body{background:red} </style><script>",
        "body{color:\\72 ed}",
        "a{b:c} /* open",
        "a{background:http://evil.example/x}",
        "a{-moz-binding:foo}",
        "a{width:expression(1)}",
        "a{\u{0}}",
        "a{content:\"/*\"} b{background:url(//evil.example/p.png)} c{content:\"*/\"}",
        "a{content:\"/*\"} @import 'x'; c{content:\"*/\"}",
    ] {
        assert!(
            matches!(
                css_presentation(css).validate(),
                Err(LogError::InvalidPresentation(_))
            ),
            "should reject {css:?}"
        );
    }
}

#[test]
fn stylesheet_rejects_oversize() {
    let ok = format!("a{{b:c}}{}", " ".repeat(MAX_STYLESHEET_BYTES - 6));
    assert_eq!(ok.len(), MAX_STYLESHEET_BYTES);
    assert!(css_presentation(&ok).validate().is_ok());
    let big = format!("{ok} ");
    assert!(css_presentation(&big).validate().is_err());
}
