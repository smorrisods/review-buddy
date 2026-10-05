//! `--json <fields>` and `--jq <expr>`: field selection over `serde_json` values, and an
//! embedded jq (`jaq`) so no `jq` binary is needed.

use jaq_core::load::{Arena, File, Loader};
use jaq_core::{data, unwrap_valr, Compiler, Ctx, Vars};
use jaq_json::Val;
use serde_json::{Map, Value};

use crate::cli::GlobalArgs;
use crate::cmd::error::CmdError;

/// What the JSON flags ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPlan {
    /// Neither flag was given: print the normal human or TSV output.
    Off,
    /// `--json` with no fields and no `--jq`: list the available fields.
    ListFields,
    /// Print the chosen fields (every field when `fields` is `None`), optionally through jq.
    Emit {
        fields: Option<Vec<String>>,
        jq: Option<String>,
    },
}

pub fn plan(json: Option<&[String]>, jq: Option<&str>) -> JsonPlan {
    let fields: Option<Vec<String>> = json.map(|f| {
        f.iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });
    match (fields, jq) {
        (None, None) => JsonPlan::Off,
        (Some(f), None) if f.is_empty() => JsonPlan::ListFields,
        (f, jq) => JsonPlan::Emit {
            fields: f.filter(|f| !f.is_empty()),
            jq: jq.map(str::to_string),
        },
    }
}

/// One field name per line, for `--json` with no fields.
pub fn list_fields(available: &[&str]) -> String {
    let mut out = available.join("\n");
    out.push('\n');
    out
}

/// Keeps only the named fields, in the order asked for. Arrays are projected element by element.
pub fn project(value: &Value, available: &[&str], fields: &[String]) -> Result<Value, CmdError> {
    if let Some(unknown) = fields.iter().find(|f| !available.contains(&f.as_str())) {
        return Err(CmdError::usage(format!(
            "Unknown field `{unknown}`. Available fields: {}.\nRun with --json and no fields to list them.",
            available.join(", ")
        )));
    }
    Ok(pick(value, fields))
}

fn pick(value: &Value, fields: &[String]) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(|v| pick(v, fields)).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for field in fields {
                if let Some(v) = map.get(field) {
                    out.insert(field.clone(), v.clone());
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Runs a jq expression over `value`. Strings come out raw (as in `gh`); everything else is
/// compact JSON. One output line per result.
pub fn run_jq(expr: &str, value: &Value) -> Result<Vec<String>, CmdError> {
    let bad = |detail: &str| {
        CmdError::usage(format!(
            "Couldn't use the jq expression `{expr}`: {detail}.\nCheck the syntax, e.g. --jq '.[].title'."
        ))
    };
    let text = serde_json::to_string(value)?;
    let input = jaq_json::read::parse_single(text.as_bytes())
        .map_err(|e| CmdError::failed(format!("Couldn't hand the data to jq: {e}.")))?;

    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let funs = jaq_core::funs()
        .chain(jaq_std::funs())
        .chain(jaq_json::funs());
    let loader = Loader::new(defs);
    let arena = Arena::default();
    let modules = loader
        .load(
            &arena,
            File {
                code: expr,
                path: (),
            },
        )
        .map_err(|_| bad("it doesn't parse"))?;
    let filter = Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|_| bad("it refers to something jq doesn't know"))?;

    let ctx = Ctx::<data::JustLut<Val>>::new(&filter.lut, Vars::new([]));
    let mut lines = Vec::new();
    for result in filter.id.run((ctx, input)).map(unwrap_valr) {
        match result {
            Ok(Val::TStr(bytes)) => lines.push(String::from_utf8_lossy(&bytes).into_owned()),
            Ok(other) => lines.push(other.to_string()),
            Err(e) => return Err(bad(&e.to_string())),
        }
    }
    Ok(lines)
}

/// Builds the text to print for the JSON flags, or `None` when they weren't used.
///
/// `full` carries every field; `available` names them in the order `--json` lists them.
/// `pretty` indents the JSON (used on a terminal).
pub fn render(
    args: &GlobalArgs,
    available: &[&str],
    full: &Value,
    pretty: bool,
) -> Option<Result<String, CmdError>> {
    let plan = plan(args.json.as_deref(), args.jq.as_deref());
    Some(match plan {
        JsonPlan::Off => return None,
        JsonPlan::ListFields => Ok(list_fields(available)),
        JsonPlan::Emit { fields, jq } => render_emit(full, available, fields, jq, pretty),
    })
}

fn render_emit(
    full: &Value,
    available: &[&str],
    fields: Option<Vec<String>>,
    jq: Option<String>,
    pretty: bool,
) -> Result<String, CmdError> {
    let chosen = match fields {
        Some(fields) => project(full, available, &fields)?,
        None => full.clone(),
    };
    let mut out = match jq {
        Some(expr) => run_jq(&expr, &chosen)?.join("\n"),
        None if pretty => serde_json::to_string_pretty(&chosen)?,
        None => serde_json::to_string(&chosen)?,
    };
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const AVAILABLE: &[&str] = &["number", "title", "author"];

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plans() {
        assert_eq!(plan(None, None), JsonPlan::Off);
        assert_eq!(plan(Some(&strings(&[""])), None), JsonPlan::ListFields);
        assert_eq!(plan(Some(&[]), None), JsonPlan::ListFields);
        assert_eq!(
            plan(Some(&strings(&["number", " title "])), None),
            JsonPlan::Emit {
                fields: Some(strings(&["number", "title"])),
                jq: None
            }
        );
        assert_eq!(
            plan(None, Some(".")),
            JsonPlan::Emit {
                fields: None,
                jq: Some(".".into())
            }
        );
        assert_eq!(
            plan(Some(&strings(&[""])), Some(".x")),
            JsonPlan::Emit {
                fields: None,
                jq: Some(".x".into())
            }
        );
    }

    #[test]
    fn listing_is_one_field_per_line() {
        assert_eq!(list_fields(AVAILABLE), "number\ntitle\nauthor\n");
    }

    #[test]
    fn projection_orders_and_filters_fields() {
        let one = json!({"author": "a", "number": 1, "title": "t"});
        assert_eq!(
            serde_json::to_string(
                &project(&one, AVAILABLE, &strings(&["title", "number"])).unwrap()
            )
            .unwrap(),
            r#"{"title":"t","number":1}"#
        );
        let many = json!([{"number": 1, "title": "a"}, {"number": 2, "title": "b"}]);
        assert_eq!(
            project(&many, AVAILABLE, &strings(&["number"])).unwrap(),
            json!([{"number": 1}, {"number": 2}])
        );
    }

    #[test]
    fn unknown_fields_are_usage_errors_listing_the_choices() {
        let err = project(&json!({}), AVAILABLE, &strings(&["nope"])).unwrap_err();
        assert_eq!(err.exit().code(), 2);
        let text = err.to_string();
        assert!(text.contains("`nope`") && text.contains("number, title, author"));
    }

    #[test]
    fn jq_filters_and_formats() {
        let data = json!([{"number": 1, "title": "One"}, {"number": 2, "title": "Two"}]);
        assert_eq!(run_jq(".[].title", &data).unwrap(), vec!["One", "Two"]);
        assert_eq!(run_jq("map(.number) | add", &data).unwrap(), vec!["3"]);
        assert_eq!(
            run_jq(".[0]", &data).unwrap(),
            vec![r#"{"number":1,"title":"One"}"#]
        );
        assert_eq!(
            run_jq(r#"[.[] | select(.number > 1)] | length"#, &data).unwrap(),
            vec!["1"]
        );
        assert!(run_jq(".[] | empty", &data).unwrap().is_empty());
    }

    #[test]
    fn bad_jq_is_a_calm_usage_error() {
        for expr in ["..[", ".a |", "nosuchfn", ".[] | error(\"boom\")"] {
            let err = run_jq(expr, &json!([1])).unwrap_err();
            assert_eq!(err.exit().code(), 2, "{expr}");
            assert!(
                err.to_string().contains("Couldn't use the jq expression"),
                "{expr}"
            );
        }
    }

    fn args(json: Option<&[&str]>, jq: Option<&str>) -> GlobalArgs {
        GlobalArgs {
            json: json.map(strings),
            jq: jq.map(str::to_string),
            ..GlobalArgs::default()
        }
    }

    #[test]
    fn render_handles_each_mode() {
        let full = json!([{"number": 1, "title": "One", "author": "a"}]);
        assert!(render(&args(None, None), AVAILABLE, &full, false).is_none());
        assert_eq!(
            render(&args(Some(&[""]), None), AVAILABLE, &full, false)
                .unwrap()
                .unwrap(),
            "number\ntitle\nauthor\n"
        );
        assert_eq!(
            render(&args(Some(&["number"]), None), AVAILABLE, &full, false)
                .unwrap()
                .unwrap(),
            "[{\"number\":1}]\n"
        );
        let pretty = render(&args(Some(&["number"]), None), AVAILABLE, &full, true)
            .unwrap()
            .unwrap();
        assert!(pretty.contains("\n  {"));
        assert_eq!(
            render(&args(None, Some(".[].title")), AVAILABLE, &full, false)
                .unwrap()
                .unwrap(),
            "One\n"
        );
        assert_eq!(
            render(
                &args(Some(&["number", "title"]), Some(".[0].title")),
                AVAILABLE,
                &full,
                false
            )
            .unwrap()
            .unwrap(),
            "One\n"
        );
        assert!(
            render(&args(Some(&["bogus"]), None), AVAILABLE, &full, false)
                .unwrap()
                .is_err()
        );
    }
}
