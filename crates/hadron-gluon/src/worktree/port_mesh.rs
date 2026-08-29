use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Mutex;
use std::net::TcpListener;

#[derive(Debug, Clone)]
pub struct PortAllocation {
    pub worktree_id: String,
    pub bindings: HashMap<String, u16>,
}

pub struct PortMesh {
    range: Range<u16>,
    allocated: Mutex<HashMap<String, HashMap<String, u16>>>,
}

impl PortMesh {
    pub fn new(range: Range<u16>) -> Self {
        Self {
            range,
            allocated: Mutex::new(HashMap::new()),
        }
    }

    pub fn allocate(&self, worktree_id: &str, names: &[&str]) -> Result<PortAllocation, String> {
        let mut map = self.allocated.lock().unwrap();
        let mut used: HashSet<u16> = map.values().flat_map(|v| v.values().copied()).collect();
        let mut result = HashMap::new();

        for name in names {
            let mut found = None;
            for port in self.range.clone() {
                if !used.contains(&port) && TcpListener::bind(("127.0.0.1", port)).is_ok() {
                    found = Some(port);
                    used.insert(port);
                    break;
                }
            }
            match found {
                Some(p) => {
                    result.insert(name.to_string(), p);
                }
                None => return Err(format!("No available ephemeral port for {}", name)),
            }
        }
        map.insert(worktree_id.to_string(), result.clone());
        Ok(PortAllocation {
            worktree_id: worktree_id.to_string(),
            bindings: result,
        })
    }

    pub fn release(&self, worktree_id: &str) {
        let mut map = self.allocated.lock().unwrap();
        map.remove(worktree_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_allocation_and_collision_avoidance() {
        let mesh = PortMesh::new(41000..41010);
        let alloc_a = mesh.allocate("wt-1", &["HTTP", "DB"]).unwrap();
        assert_eq!(alloc_a.bindings.len(), 2);
        assert!(alloc_a.bindings.get("HTTP").unwrap() >= &41000);

        let alloc_b = mesh.allocate("wt-2", &["HTTP"]).unwrap();
        assert_ne!(
            alloc_a.bindings.get("HTTP"),
            alloc_b.bindings.get("HTTP")
        );

        mesh.release("wt-1");
        let alloc_c = mesh.allocate("wt-3", &["HTTP"]).unwrap();
        assert!(alloc_c.bindings.contains_key("HTTP"));
    }
}
