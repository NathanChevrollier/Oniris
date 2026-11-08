//! Cœur de recherche : index mémoire des fichiers, classement flou, fréquence d'usage.
mod index;
mod rank;
mod usage;

pub use index::{norm_key, under_any, FileIndex};
pub use rank::{frecency, query, query_at, Hit, FRECENCY_WEIGHT};
pub use usage::{bump, bumped, load, prune, prune_stale, record, UsageMap};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    File,
    Dir,
    App,
    Project,
    Favorite,
    Note,
    Command,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Dir => "dir",
            Self::App => "app",
            Self::Project => "project",
            Self::Favorite => "favorite",
            Self::Note => "note",
            Self::Command => "command",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "file" => Self::File,
            "dir" => Self::Dir,
            "app" => Self::App,
            "project" => Self::Project,
            "favorite" => Self::Favorite,
            "note" => Self::Note,
            "command" => Self::Command,
            _ => return None,
        })
    }
}

/// `key` : chemin, url ou id, unique par `kind`. Pour File/Dir c'est le chemin complet
/// (contenu utilisateur : ne jamais le journaliser).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    pub kind: ItemKind,
    pub key: String,
    pub title: String,
    pub subtitle: String,
}
