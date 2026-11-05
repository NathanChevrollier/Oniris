//! Mails Gmail : client REST, synchronisation incrémentale et cache des en-têtes (migration 14).
//! Le corps n'est jamais stocké en base. Journal : comptes, nombres et statuts seulement.
pub mod gmail;
pub mod html;
pub mod mime;
pub mod sync;
pub mod tools;

use serde::Serialize;

/// Tri d'un mail : `todo` (à traiter), `read` (à lire), `filtered` (promotions, listes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Bucket { Todo, Read, Filtered }

impl Bucket {
    pub fn as_str(self) -> &'static str {
        match self { Bucket::Todo => "todo", Bucket::Read => "read", Bucket::Filtered => "filtered" }
    }
}

const FILTERED: [&str; 4] = ["CATEGORY_PROMOTIONS", "CATEGORY_SOCIAL", "CATEGORY_UPDATES", "CATEGORY_FORUMS"];

/// Tri par défaut (`bucket_src = 'heuristic'`), remplacé plus tard par IRIS.
pub fn heuristic_bucket(labels: &[String], has_list_unsubscribe: bool) -> Bucket {
    if has_list_unsubscribe || labels.iter().any(|l| FILTERED.contains(&l.as_str())) {
        Bucket::Filtered
    } else if labels.iter().any(|l| l == "IMPORTANT") {
        Bucket::Todo
    } else {
        Bucket::Read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn tri_heuristique() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let cases: [(&[&str], bool, Bucket); 9] = [
            (&["INBOX", "CATEGORY_PROMOTIONS"], false, Bucket::Filtered),
            (&["INBOX", "CATEGORY_SOCIAL", "IMPORTANT"], false, Bucket::Filtered),
            (&["INBOX", "CATEGORY_UPDATES"], false, Bucket::Filtered),
            (&["INBOX", "CATEGORY_FORUMS"], false, Bucket::Filtered),
            (&["INBOX", "IMPORTANT"], true, Bucket::Filtered),
            (&["INBOX", "IMPORTANT", "CATEGORY_PERSONAL"], false, Bucket::Todo),
            (&["INBOX", "CATEGORY_PERSONAL"], false, Bucket::Read),
            (&["INBOX", "UNREAD"], false, Bucket::Read),
            (&[], false, Bucket::Read),
        ];
        for (labels, unsub, want) in cases {
            assert_eq!(heuristic_bucket(&l(labels), unsub), want, "{labels:?} {unsub}");
        }
        assert_eq!(serde_json::to_string(&Bucket::Filtered).unwrap(), "\"filtered\"");
    }
}
