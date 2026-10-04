//! Classification d'âge OARS (comme `as-content-rating.c` de libappstream) :
//! attributs `content_rating` → âge CSM minimal, formaté selon le système
//! régional (PEGI en Europe, IARC ailleurs), + descriptions humaines.

/// (id, [none, mild, moderate, intense]) — âge CSM par valeur. Table verbatim
/// de libappstream (OARS 1.0 + 1.1).
const CSM: &[(&str, [u32; 4])] = &[
    ("violence-cartoon", [0, 3, 4, 6]),
    ("violence-fantasy", [0, 3, 7, 8]),
    ("violence-realistic", [0, 4, 9, 14]),
    ("violence-bloodshed", [0, 9, 11, 18]),
    ("violence-sexual", [0, 18, 18, 18]),
    ("drugs-alcohol", [0, 11, 13, 16]),
    ("drugs-narcotics", [0, 12, 14, 17]),
    ("drugs-tobacco", [0, 10, 13, 13]),
    ("sex-nudity", [0, 12, 14, 14]),
    ("sex-themes", [0, 13, 14, 15]),
    ("language-profanity", [0, 8, 11, 14]),
    ("language-humor", [0, 3, 8, 14]),
    ("language-discrimination", [0, 9, 10, 11]),
    ("money-advertising", [0, 7, 8, 10]),
    ("money-gambling", [0, 7, 10, 18]),
    ("money-purchasing", [0, 12, 14, 15]),
    ("social-chat", [0, 4, 10, 13]),
    ("social-audio", [0, 15, 15, 15]),
    ("social-contacts", [0, 12, 12, 12]),
    ("social-info", [0, 0, 13, 13]),
    ("social-location", [0, 13, 13, 13]),
    ("sex-homosexuality", [0, 13, 14, 15]),
    ("sex-prostitution", [0, 12, 14, 18]),
    ("sex-adultery", [0, 8, 10, 18]),
    ("sex-appearance", [0, 10, 10, 15]),
    ("violence-worship", [0, 13, 15, 18]),
    ("violence-desecration", [0, 13, 15, 18]),
    ("violence-slavery", [0, 13, 15, 18]),
];

/// (id, [none, mild, moderate, intense]) — description humaine (verbatim
/// libappstream, EN comme GNOME les affiche). `None` = valeur non définie.
const DESC: &[(&str, [Option<&str>; 4])] = &[
    ("violence-cartoon", [Some("No cartoon violence"), Some("Cartoon characters in unsafe situations"), Some("Cartoon characters in aggressive conflict"), Some("Graphic violence involving cartoon characters")]),
    ("violence-fantasy", [Some("No fantasy violence"), Some("Characters in unsafe situations easily distinguishable from reality"), Some("Characters in aggressive conflict easily distinguishable from reality"), Some("Graphic violence easily distinguishable from reality")]),
    ("violence-realistic", [Some("No realistic violence"), Some("Mildly realistic characters in unsafe situations"), Some("Depictions of realistic characters in aggressive conflict"), Some("Graphic violence involving realistic characters")]),
    ("violence-bloodshed", [Some("No bloodshed"), Some("Unrealistic bloodshed"), Some("Realistic bloodshed"), Some("Depictions of bloodshed and the mutilation of body parts")]),
    ("violence-sexual", [Some("No sexual violence"), None, None, Some("Rape or other violent sexual behavior")]),
    ("drugs-alcohol", [Some("No references to alcohol"), Some("References to alcoholic beverages"), Some("Use of alcoholic beverages"), None]),
    ("drugs-narcotics", [Some("No references to illicit drugs"), Some("References to illicit drugs"), Some("Use of illicit drugs"), None]),
    ("drugs-tobacco", [Some("No references to tobacco products"), Some("References to tobacco products"), Some("Use of tobacco products"), None]),
    ("sex-nudity", [Some("No nudity of any sort"), Some("Brief artistic nudity"), Some("Prolonged nudity"), Some("Explicit nudity involving visible sexual organs")]),
    ("sex-themes", [Some("No references to or depictions of sexual nature"), Some("Provocative references or depictions"), Some("Sexual references or depictions"), Some("Graphic sexual behavior")]),
    ("language-profanity", [Some("No profanity of any kind"), Some("Mild or infrequent use of profanity"), Some("Moderate use of profanity"), Some("Strong or frequent use of profanity")]),
    ("language-humor", [Some("No inappropriate humor"), Some("Slapstick humor"), Some("Vulgar or bathroom humor"), Some("Mature or sexual humor")]),
    ("language-discrimination", [Some("No discriminatory language of any kind"), Some("Negativity towards a specific group of people"), Some("Discrimination designed to cause emotional harm"), Some("Explicit discrimination based on gender, sexuality, race or religion")]),
    ("money-advertising", [Some("No advertising of any kind"), Some("Product placement"), Some("Explicit references to specific brands or trademarked products"), Some("Users are encouraged to purchase specific real-world items")]),
    ("money-gambling", [Some("No gambling of any kind"), Some("Gambling on random events using tokens or credits"), Some("Gambling using \"play\" money"), Some("Gambling using real money")]),
    ("money-purchasing", [Some("No ability to spend money"), Some("Users are encouraged to donate real money"), None, Some("Ability to spend real money in-app")]),
    ("social-chat", [Some("No way to chat with other users"), Some("User-to-user interactions without chat functionality"), Some("Moderated chat functionality between users"), Some("Uncontrolled chat functionality between users")]),
    ("social-audio", [Some("No way to talk with other users"), None, Some("Moderated audio or video chat functionality between users"), Some("Uncontrolled audio or video chat functionality between users")]),
    ("social-contacts", [Some("No sharing of social network usernames or email addresses"), None, None, Some("Sharing social network usernames or email addresses")]),
    ("social-info", [Some("No sharing of user information with third parties"), Some("Checking for the latest application version"), Some("Sharing diagnostic data that does not let others identify the user"), Some("Sharing information that lets others identify the user")]),
    ("social-location", [Some("No sharing of physical location with other users"), None, None, Some("Sharing physical location with other users")]),
];

/// Territoires utilisant PEGI (sous-ensemble européen ; sinon IARC).
const PEGI: &[&str] = &[
    "AL", "AD", "AT", "BE", "BA", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR",
    "DE", "GR", "HU", "IS", "IE", "IL", "IT", "LV", "LI", "LT", "LU", "MT", "MD",
    "MC", "ME", "NL", "MK", "NO", "PL", "PT", "RO", "RS", "SK", "SI", "ES", "SE",
    "CH", "UA", "GB", "ZA",
];

fn value_index(v: &str) -> usize {
    match v {
        "mild" => 1,
        "moderate" => 2,
        "intense" => 3,
        _ => 0, // "none" ou inconnu
    }
}

fn territory() -> Option<String> {
    for var in ["LC_ALL", "LANG", "LANGUAGE"] {
        if let Ok(v) = std::env::var(var) {
            // ex. "fr_FR.UTF-8" → "FR"
            if let Some((_, rest)) = v.split_once('_') {
                let terr: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
                if !terr.is_empty() {
                    return Some(terr.to_uppercase());
                }
            }
        }
    }
    None
}

/// Âge affiché (« 12 » PEGI, « 12+ » IARC) + descriptions triées par sévérité
/// décroissante. `None` si aucun attribut OARS renseigné.
pub fn rating(oars: &[(String, String)]) -> Option<(String, Vec<String>)> {
    if oars.is_empty() {
        return None;
    }

    let csm = |id: &str, idx: usize| CSM.iter().find(|(i, _)| *i == id).map(|(_, a)| a[idx]).unwrap_or(0);

    // Âge minimal = max des âges CSM des attributs renseignés.
    let min_age = oars
        .iter()
        .map(|(id, v)| csm(id, value_index(v)))
        .max()
        .unwrap_or(0);

    // Comme GNOME : ne montre que les descriptions des attributs À l'âge maximal,
    // en ordre canonique (ordre de la table), pas l'ordre du fichier.
    let mut descs: Vec<String> = Vec::new();
    for (id, dvals) in DESC {
        if let Some((_, v)) = oars.iter().find(|(i, _)| i == id) {
            let idx = value_index(v);
            if idx != 0 && csm(id, idx) == min_age {
                if let Some(d) = dvals[idx] {
                    descs.push(d.to_string());
                }
            }
        }
    }
    Some((format_age(min_age), descs))
}

/// Formatte un âge CSM selon le système régional : PEGI (« 12 ») ou IARC (« 12+ »).
fn format_age(age: u32) -> String {
    let pegi = territory().map(|t| PEGI.contains(&t.as_str())).unwrap_or(false);
    let bucket = if age >= 18 {
        18
    } else if age >= 16 {
        16
    } else if age >= 12 {
        12
    } else if age >= 7 {
        7
    } else {
        3
    };
    if pegi {
        bucket.to_string()
    } else {
        format!("{bucket}+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn steam_rating_is_15_csm() {
        // social-chat=13, social-info=13, social-audio=15, money-purchasing=15 → max 15.
        let o = oars(&[
            ("social-chat", "intense"),
            ("social-info", "intense"),
            ("social-audio", "intense"),
            ("money-purchasing", "intense"),
        ]);
        let _g = crate::env_lock();
        std::env::set_var("LC_ALL", "fr_FR.UTF-8");
        let (age, descs) = rating(&o).unwrap();
        assert_eq!(age, "12", "PEGI (FR) : 15 → bucket 12");
        // Descriptions triées par sévérité : les âge-15 (money/audio) en tête.
        assert_eq!(descs[0], "Ability to spend real money in-app");
        assert_eq!(descs[1], "Uncontrolled audio or video chat functionality between users");
        std::env::remove_var("LC_ALL");
    }

    #[test]
    fn iarc_formatting_for_non_pegi() {
        let o = oars(&[("money-purchasing", "intense")]); // age 15
        let _g = crate::env_lock();
        std::env::remove_var("LC_ALL");
        std::env::set_var("LANG", "en_US.UTF-8");
        let (age, _) = rating(&o).unwrap();
        assert_eq!(age, "12+", "IARC (US) : 15 → 12+");
        std::env::remove_var("LANG");
    }

    #[test]
    fn none_values_ignored() {
        let o = oars(&[("violence-cartoon", "none"), ("social-chat", "none")]);
        let _g = crate::env_lock(); // `rating` lit LC_ALL/LANG (format_age)
        let (age, descs) = rating(&o).unwrap();
        assert!(descs.is_empty());
        // âge 0 → bucket 3
        assert!(age.starts_with('3'));
    }

    #[test]
    fn empty_is_none() {
        assert!(rating(&[]).is_none());
    }
}
