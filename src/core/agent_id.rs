use crate::crc64::crc64_ecma;

pub fn compute_agent_id(host: &str, deploy_dir: &str) -> i64 {
    crc64_ecma(&format!("{host}_{deploy_dir}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_agent_id_deterministic() {
        let id = compute_agent_id("192.168.1.100", "/opt/agent/1");
        assert_eq!(id, 6710796062293130245);
    }

    #[test]
    fn test_compute_agent_id_uniqueness() {
        let id1 = compute_agent_id("10.0.0.1", "/opt/a");
        let id2 = compute_agent_id("10.0.0.1", "/opt/b");
        assert_ne!(id1, id2, "different paths should differ");
        let id3 = compute_agent_id("10.0.0.2", "/opt/a");
        assert_ne!(id1, id3, "different IPs should differ");
    }
}
