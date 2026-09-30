//! Translation cache: records plus (in `skill_store`) their persistence.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationRecord {
    pub fingerprint: String,
    pub kind: String,
    pub source_name: String,
    pub zh_name: String,
    pub zh_description: String,
    pub model: String,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::SkillStore;

    fn record(fp: &str) -> TranslationRecord {
        TranslationRecord {
            fingerprint: fp.into(),
            kind: "plugin_skill".into(),
            source_name: "brainstorming".into(),
            zh_name: "头脑风暴".into(),
            zh_description: "写代码前的需求与设计探索".into(),
            model: "test-model".into(),
            created_at: "2026-10-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn upsert_get_clear_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();

        assert!(store.get_translations().unwrap().is_empty());

        store.upsert_translations(&[record("fp1"), record("fp2")]).unwrap();
        let map = store.get_translations().unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["fp1"].zh_name, "头脑风暴");

        // Upsert overwrites by fingerprint
        let mut updated = record("fp1");
        updated.zh_name = "脑暴".into();
        store.upsert_translations(&[updated]).unwrap();
        assert_eq!(store.get_translations().unwrap()["fp1"].zh_name, "脑暴");
        assert_eq!(store.get_translations().unwrap().len(), 2);

        store.clear_translations().unwrap();
        assert!(store.get_translations().unwrap().is_empty());
    }
}