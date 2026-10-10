//! ODRS ratings, the review source GNOME Software uses.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Average stars, review count and per-star counts of an app.
#[derive(Debug, Clone, Copy)]
pub struct Rating {
    pub stars: f32,
    pub total: u64,
    pub star1: u64,
    pub star2: u64,
    pub star3: u64,
    pub star4: u64,
    pub star5: u64,
}

static ODRS: OnceLock<HashMap<String, Rating>> = OnceLock::new();

/// All ODRS ratings, fetched once per session.
pub(crate) fn odrs_ratings() -> &'static HashMap<String, Rating> {
    ODRS.get_or_init(|| fetch_odrs_ratings().unwrap_or_default())
}

fn fetch_odrs_ratings() -> Option<HashMap<String, Rating>> {
    let v = crate::http::get_json("https://odrs.gnome.org/1.0/reviews/api/ratings")?;
    let obj = v.as_object()?;
    let mut map = HashMap::with_capacity(obj.len());
    for (id, val) in obj {
        let get = |k: &str| val.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        let (s1, s2, s3, s4, s5) = (
            get("star1"),
            get("star2"),
            get("star3"),
            get("star4"),
            get("star5"),
        );
        let n = s1 + s2 + s3 + s4 + s5;
        if n == 0 {
            continue;
        }
        let stars = (s1 + s2 * 2 + s3 * 3 + s4 * 4 + s5 * 5) as f32 / n as f32;
        let total = val.get("total").and_then(|x| x.as_u64()).unwrap_or(n);
        let rating = Rating {
            stars,
            total,
            star1: s1,
            star2: s2,
            star3: s3,
            star4: s4,
            star5: s5,
        };
        // Index lowercase; the most rated wins.
        let key = id.strip_suffix(".desktop").unwrap_or(id).to_lowercase();
        map.entry(key)
            .and_modify(|e: &mut Rating| {
                if total > e.total {
                    *e = rating;
                }
            })
            .or_insert(rating);
    }
    Some(map)
}

/// The rating of `app_id`, ignoring case.
pub fn rating_for(app_id: &str) -> Option<Rating> {
    if app_id.is_empty() {
        return None;
    }
    odrs_ratings().get(&app_id.to_lowercase()).copied()
}
