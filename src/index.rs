use crate::model::MODEL_ID;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub struct Index {
    db: Connection,
}
impl Index {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS embeddings (hash TEXT NOT NULL, model TEXT NOT NULL, vector BLOB NOT NULL, PRIMARY KEY(hash, model));")?;
        Ok(Self { db })
    }
    pub fn get(&self, hash: &str) -> Result<Option<Vec<f32>>> {
        let bytes: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT vector FROM embeddings WHERE hash=?1 AND model=?2",
                params![hash, MODEL_ID],
                |r| r.get(0),
            )
            .optional()?;
        Ok(bytes.and_then(|b| {
            if b.len() != 768 * 4 {
                return None;
            }
            let v: Vec<f32> = b
                .as_chunks::<4>()
                .0
                .iter()
                .map(|s| f32::from_le_bytes(*s))
                .collect();
            v.iter().all(|x| x.is_finite()).then_some(v)
        }))
    }
    pub fn put(&self, hash: &str, vector: &[f32]) -> Result<()> {
        anyhow::ensure!(
            vector.len() == 768 && vector.iter().all(|v| v.is_finite()),
            "Invalid embedding"
        );
        let bytes: Vec<u8> = vector.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.db.execute(
            "INSERT OR REPLACE INTO embeddings(hash,model,vector) VALUES(?1,?2,?3)",
            params![hash, MODEL_ID, bytes],
        )?;
        Ok(())
    }
}
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return f32::NEG_INFINITY;
    }
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
/// Score each vector once, then sort only the best matches. Index order breaks ties.
pub fn rank(vectors: &[Option<Vec<f32>>], query: &[f32], limit: usize) -> Vec<usize> {
    if limit == 0 {
        return vec![];
    }
    let mut scores: Vec<_> = vectors
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.as_ref().map(|v| (i, cosine(v, query))))
        .collect();
    let compare = |a: &(usize, f32), b: &(usize, f32)| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0));
    if scores.len() > limit {
        scores.select_nth_unstable_by(limit, compare);
        scores.truncate(limit);
    }
    scores.sort_unstable_by(compare);
    scores.into_iter().map(|(i, _)| i).collect()
}
pub fn uuid_query(query: &str) -> Option<String> {
    let q = query.trim().to_lowercase();
    let compact = q.replace('-', "");
    // Require at least 8 hex digits so words like 'bed' remain semantic queries.
    (compact.len() >= 8 && compact.len() <= 32 && compact.bytes().all(|c| c.is_ascii_hexdigit()))
        .then_some(compact)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn top_matches_preserve_full_sort_order_and_ties() {
        let vectors: Vec<_> = (0..1000)
            .map(|i| {
                if i % 7 == 0 {
                    None
                } else {
                    Some(vec![(i % 37) as f32, 1.])
                }
            })
            .collect();
        let query = [1., 0.];
        let mut expected: Vec<_> = (0..vectors.len())
            .filter(|&i| vectors[i].is_some())
            .collect();
        expected.sort_by(|&a, &b| {
            cosine(vectors[b].as_ref().unwrap(), &query)
                .total_cmp(&cosine(vectors[a].as_ref().unwrap(), &query))
        });
        for limit in [0, 1, 200, 2000] {
            assert_eq!(
                rank(&vectors, &query, limit),
                expected.iter().take(limit).copied().collect::<Vec<_>>()
            );
        }
    }
    #[test]
    fn persistence_and_queries() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("index.db");
        let v = crate::model::normalize(vec![1.; 768]).unwrap();
        Index::open(&path).unwrap().put("abc", &v).unwrap();
        assert_eq!(Index::open(&path).unwrap().get("abc").unwrap().unwrap(), v);
        assert!(
            Index::open(&path)
                .unwrap()
                .get("changed")
                .unwrap()
                .is_none()
        );
        assert!(uuid_query("deadbeef-1234").is_some());
        assert!(uuid_query("red bed").is_none());
        assert!(uuid_query("bed").is_none());
        assert!(cosine(&v, &v) > 0.999);
    }
}
