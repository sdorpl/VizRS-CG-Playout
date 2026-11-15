//! Most strona → okno: komunikaty `app:` panelu (biała lista, limity) – ten sam protokół co okna VizRS CG System
//! i VizRS CG Artist (`window.__vizrsPost('app:' + JSON)`). Panel może tylko zmienić tytuł własnego okna
//! i poprosić system o uwagę (alarm); nic więcej nie przechodzi.

use serde_json::Value;

const MAX_TITLE: usize = 200;

#[derive(Debug, PartialEq)]
pub enum Msg {
    /// `alert` – alarm panelu: okno w tle miga w pasku zadań / podskakuje w Docku.
    State { title: String, alert: bool },
}

pub fn parse(body: &str) -> Option<Msg> {
    if body.len() > 4096 {
        return None;
    }
    let v: Value = serde_json::from_str(body).ok()?;
    match v["t"].as_str()? {
        "state" => {
            let title: String = v["title"]
                .as_str()?
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_TITLE)
                .collect();
            Some(Msg::State {
                title,
                alert: v["alert"].as_bool().unwrap_or(false),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_list_and_limits() {
        assert_eq!(
            parse(r#"{"t":"state","title":"Na antenie","alert":true}"#),
            Some(Msg::State {
                title: "Na antenie".into(),
                alert: true
            })
        );
        let long = format!(r#"{{"t":"state","title":"{}"}}"#, "x".repeat(500));
        assert!(matches!(parse(&long), Some(Msg::State { title, .. }) if title.len() == MAX_TITLE));
        assert_eq!(
            parse(r#"{"t":"state","title":"a\nb"}"#),
            Some(Msg::State {
                title: "ab".into(),
                alert: false
            })
        );
        assert_eq!(parse(r#"{"t":"draft","id":"x","data":"y"}"#), None);
        assert_eq!(parse(r#"{"t":"state"}"#), None);
        assert_eq!(parse("nie json"), None);
        assert_eq!(parse(&"x".repeat(5000)), None);
    }
}
