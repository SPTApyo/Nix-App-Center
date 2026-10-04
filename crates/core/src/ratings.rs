//! ODRS ratings (odrs.gnome.org), the same community review source as GNOME
//! Software, indexed by app id (AppStream/desktop). Purely cosmetic.

use std::collections::HashMap;
use std::sync::OnceLock;

/// An app rating: average stars (0–5) + total review count + per-star
/// histogram (for the "Ratings & Reviews" block, like GsReviewHistogram).
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

/// Full ODRS ratings table, fetched once (bulk) then cached for the whole
/// session.
pub(crate) fn odrs_ratings() -> &'static HashMap<String, Rating> {
    ODRS.get_or_init(|| fetch_odrs_ratings().unwrap_or_default())
}

fn fetch_odrs_ratings() -> Option<HashMap<String, Rating>> {
    let v = crate::http::get_json("https://odrs.gnome.org/1.0/reviews/api/ratings")?;
    let obj = v.as_object()?;
    let mut map = HashMap::with_capacity(obj.len());
    for (id, val) in obj {
        let get = |k: &str| val.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        let (s1, s2, s3, s4, s5) =
            (get("star1"), get("star2"), get("star3"), get("star4"), get("star5"));
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
        // ODRS keys are desktop ids, sometimes cased differently from Flathub
        // (`org.mozilla.Firefox` vs `org.mozilla.firefox`). So they are indexed
        // lowercase, without the `.desktop` suffix, keeping the most rated
        // entry on collision.
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

/// ODRS rating for a Flathub `app_id` (case-insensitive match).
pub fn rating_for(app_id: &str) -> Option<Rating> {
    if app_id.is_empty() {
        return None;
    }
    odrs_ratings().get(&app_id.to_lowercase()).copied()
}
