/// Lightweight k-means clustering for semantic result grouping.
///
/// Groups search hits by vector proximity and labels each cluster from
/// the most common title/category token in its members.
use crate::model::Hit;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Cluster {
    pub label: String,
    pub count: usize,
    /// Product IDs in this cluster.
    pub ids: Vec<String>,
}

/// Cluster `hits` using their stored product vectors.
///
/// `vectors` must be parallel to `hits` (same length, same order).
/// Silently returns an empty vec if all vectors are missing or dim-mismatched.
pub fn cluster_hits(hits: &[Hit], vectors: &[Option<Vec<f32>>], k: usize) -> Vec<Cluster> {
    let k = k.clamp(2, 10);

    // Build index of hits that have a vector.
    let indexed: Vec<(usize, &Vec<f32>)> = vectors
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.as_ref().map(|vec| (i, vec)))
        .collect();

    if indexed.len() < k {
        return vec![];
    }

    let dims = indexed[0].1.len();

    // Initialize centroids as the first k vectors.
    let mut centroids: Vec<Vec<f32>> = indexed[..k].iter().map(|(_, v)| (*v).clone()).collect();
    let mut assignments = vec![0usize; indexed.len()];

    // 3 iterations of k-means is enough for display-quality clusters.
    for _ in 0..3 {
        // Assignment step.
        for (j, (_, vec)) in indexed.iter().enumerate() {
            let nearest = centroids
                .iter()
                .enumerate()
                .map(|(ci, c)| (ci, cosine_distance(vec, c)))
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(ci, _)| ci)
                .unwrap_or(0);
            assignments[j] = nearest;
        }

        // Update step.
        for ci in 0..k {
            let members: Vec<&Vec<f32>> = indexed
                .iter()
                .zip(assignments.iter())
                .filter(|&(_, &a)| a == ci)
                .map(|((_, v), _)| *v)
                .collect();
            if members.is_empty() {
                continue;
            }
            let mut new_centroid = vec![0.0f32; dims];
            for v in &members {
                if v.len() == dims {
                    for (s, x) in new_centroid.iter_mut().zip(v.iter()) {
                        *s += x;
                    }
                }
            }
            let n = members.len() as f32;
            for s in &mut new_centroid {
                *s /= n;
            }
            centroids[ci] = new_centroid;
        }
    }

    // Build output clusters.
    let mut clusters: Vec<Cluster> = (0..k).map(|_| Cluster { label: String::new(), count: 0, ids: vec![] }).collect();
    for (j, (hit_idx, _)) in indexed.iter().enumerate() {
        let ci = assignments[j];
        clusters[ci].count += 1;
        clusters[ci].ids.push(hits[*hit_idx].id.clone());
    }

    // Label each cluster from the most frequent meaningful token in metadata titles.
    for (ci, cluster) in clusters.iter_mut().enumerate() {
        let _ = ci;
        let mut token_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for id in &cluster.ids {
            if let Some(hit) = hits.iter().find(|h| &h.id == id) {
                for field in &["category", "title", "name"] {
                    if let Some(s) = hit.metadata.get(field).and_then(|v| v.as_str()) {
                        for word in s.split_whitespace() {
                            let w = word.to_lowercase();
                            if w.len() >= 4 {
                                *token_counts.entry(w).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
        }
        cluster.label = token_counts
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(w, _)| capitalize(&w))
            .unwrap_or_else(|| format!("Group {}", ci + 1));
    }

    // Drop empty clusters and sort by size descending.
    clusters.retain(|c| c.count > 0);
    clusters.sort_by(|a, b| b.count.cmp(&a.count));
    clusters
}

fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { return 1.0; }
    1.0 - (dot / (na * nb))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}
