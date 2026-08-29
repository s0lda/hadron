use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub new_local: usize,
    pub new_global: usize,
}

pub struct GlobalNucleusSync;

impl GlobalNucleusSync {
    pub fn merge_notes(
        local: &HashMap<String, String>,
        global: &HashMap<String, String>,
    ) -> (HashMap<String, String>, SyncReport) {
        let mut merged = local.clone();
        let mut report = SyncReport::default();

        for (k, v) in global {
            if !merged.contains_key(k) {
                merged.insert(k.clone(), v.clone());
                report.new_local += 1;
            }
        }
        for k in local.keys() {
            if !global.contains_key(k) {
                report.new_global += 1;
            }
        }
        (merged, report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_sync_note_merging() {
        let mut local = HashMap::new();
        local.insert("note-1".to_string(), "Local content 1".to_string());

        let mut global = HashMap::new();
        global.insert("note-2".to_string(), "Global content 2".to_string());

        let (merged, report) = GlobalNucleusSync::merge_notes(&local, &global);
        assert_eq!(merged.len(), 2);
        assert_eq!(report.new_local, 1);
        assert_eq!(report.new_global, 1);
    }
}
