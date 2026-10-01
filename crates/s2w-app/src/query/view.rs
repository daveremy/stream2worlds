//! The world as a d3 node/link graph at a level of detail, optionally around a focus entity.

use std::collections::BTreeMap;

use s2w_core::{AttrMap, EntityId, World};
use serde::Serialize;

use super::QueryError;
use super::epoch::Epoch;

mod by_type;
mod graph;
mod hubs;
mod ids;
#[cfg(test)]
mod tests;

pub use by_type::type_summary;
pub(in crate::query) use graph::Graph;
pub(in crate::query) use hubs::{HubAgg, HubFacts, NodeParts, entity_node};
pub(in crate::query) use ids::{IdDigits, LinkRef, NodeIdRef};

use by_type::type_view;
use graph::graph_view;

/// The most hops a focused view may request.
pub const MAX_HOPS: u32 = 5;

/// Level of detail. `cluster` is part of the URL vocabulary but not served yet (decision 0006).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lod {
    /// One node per entity type.
    Type,
    /// One node per (resolved) entity.
    Entity,
}

/// What to project.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ViewParams {
    /// Level of detail.
    pub lod: Lod,
    /// Restrict to the neighbourhood of this entity id (resolved through merges).
    pub focus: Option<u64>,
    /// Neighbourhood radius when `focus` is set, at most [`MAX_HOPS`].
    pub hops: u32,
    /// Whether the view carries links. [`LinkDetail::None`] is the type summary
    /// ([`type_summary`]): valid only with [`Lod::Type`] and no `focus`.
    pub links: LinkDetail,
}

impl Default for ViewParams {
    fn default() -> Self {
        Self {
            lod: Lod::Entity,
            focus: None,
            hops: 1,
            links: LinkDetail::All,
        }
    }
}

/// The `links` parameter (s2w#296): every link, or none at all.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum LinkDetail {
    /// The view with its links (the default).
    #[default]
    All,
    /// No links: at `lod=type` the type summary, built without the relationship pass.
    None,
}

/// Refuses `links=none` outside its one valid shape, `lod=type` with no `focus`.
///
/// # Errors
/// [`QueryError::BadParameter`] naming `links`.
pub(crate) fn check_links(params: &ViewParams) -> Result<(), QueryError> {
    if params.links == LinkDetail::All {
        return Ok(());
    }
    let reason = if params.lod != Lod::Type {
        "links=none needs lod=type"
    } else if params.focus.is_some() {
        "links=none cannot be combined with focus"
    } else {
        return Ok(());
    };
    Err(QueryError::BadParameter {
        name: "links",
        reason: reason.to_owned(),
    })
}

/// A relationship from a source to a hub, carried on the source instead of as a link.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct HubRef {
    /// The relationship kind.
    pub kind: String,
    /// The hub's node id, `e:<id>`.
    pub hub: String,
}

/// A graph node. Every `id` is a prefixed string (`e:<id>`, `type:<name>`), so d3's `===`
/// comparison never mixes numbers and strings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Node {
    /// A resolved entity.
    Entity {
        /// `e:<id>`.
        id: String,
        /// The entity id.
        entity: EntityId,
        /// The latest observed type; empty if only named by relationships.
        entity_type: String,
        /// Natural keys that resolve to this entity, for labels.
        keys: Vec<String>,
        /// Attributes.
        attrs: AttrMap,
        /// Ids merged into this entity, excluding itself.
        members: Vec<EntityId>,
        /// Relationships to hubs, served on the source instead of as links.
        hub_refs: Vec<HubRef>,
    },
    /// An entity past the hub in-degree cap, served as an aggregate at every level of detail:
    /// no per-source link into it is emitted.
    Hub {
        /// `e:<id>`: a hub keeps its entity id, so a URL focused on it survives the trip.
        id: String,
        /// The entity id.
        entity: EntityId,
        /// The latest observed type.
        entity_type: String,
        /// Natural keys that resolve to this entity.
        keys: Vec<String>,
        /// Attributes.
        attrs: AttrMap,
        /// Ids merged into this entity, excluding itself.
        members: Vec<EntityId>,
        /// Distinct (resolved) sources pointing at the hub.
        in_degree: u64,
        /// Observations by relationship kind.
        by_kind: BTreeMap<String, u64>,
        /// The world offset of the latest relationship observed pointing here.
        last_seen_offset: u64,
        /// The hub's own relationships into other hubs, as on [`Node::Entity`]: no link into a
        /// hub is emitted at `lod=entity`, so without this a hub-to-hub edge would vanish there.
        hub_refs: Vec<HubRef>,
    },
    /// Every non-hub entity of one type.
    Type {
        /// `type:<name>`.
        id: String,
        /// The type; `untyped` for entities only named by relationships.
        entity_type: String,
        /// How many entities.
        count: u64,
    },
}

impl Node {
    /// The d3 node id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Entity { id, .. } | Self::Hub { id, .. } | Self::Type { id, .. } => id,
        }
    }
}

/// A d3 link between two node ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Link {
    /// Source node id.
    pub source: String,
    /// Target node id.
    pub target: String,
    /// Relationship kind.
    pub kind: String,
    /// Observation count; for an aggregate link into a hub, the number of distinct sources.
    pub weight: u64,
}

/// A world snapshot in the d3 node/link shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorldView {
    /// The fold offset this view is at.
    pub offset: u64,
    /// The history `offset` belongs to (see [`Epoch`]): pass it back as `epoch` with any offset
    /// read from this view.
    pub epoch: Epoch,
    /// The world branch; only `actual` exists.
    pub branch: &'static str,
    /// The fold version that produced the world.
    pub fold_version: u32,
    /// The in-degree cap the world was folded under.
    pub hub_in_degree_cap: u64,
    /// The level of detail.
    pub lod: Lod,
    /// The focus entity id, if any.
    pub focus: Option<u64>,
    /// Nodes, sorted by id.
    pub nodes: Vec<Node>,
    /// Links, sorted by source, target, kind.
    pub links: Vec<Link>,
}

/// The name of the only branch served.
pub const ACTUAL_BRANCH: &str = "actual";

pub(crate) fn node_id(id: EntityId) -> String {
    format!("e:{}", id.get())
}

/// Projects `world` at `params`. Pure: the HTTP handler, `--json` and MCP all call this. The
/// view's `epoch` is 0 here; [`super::QueryState::view_at`] labels it with the served one.
///
/// # Errors
/// [`QueryError::UnknownEntity`] for an unknown focus, [`QueryError::HopsTooLarge`] past
/// [`MAX_HOPS`], [`QueryError::BadParameter`] for `links=none` with `lod=entity` or a focus.
pub fn world_view(world: &World, params: &ViewParams) -> Result<WorldView, QueryError> {
    check_links(params)?;
    if params.links == LinkDetail::None {
        return Ok(type_summary(world));
    }
    if params.lod == Lod::Type && params.focus.is_none() {
        return Ok(type_view(world));
    }
    graph_view(world, params)
}
