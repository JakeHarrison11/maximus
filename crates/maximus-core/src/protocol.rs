//! Frames shared by native and browser Maxima transports.

use crate::backend::*;

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' {
            match chars.next() {
                Some('n') => '\n',
                Some('r') => '\r',
                Some('t') => '\t',
                Some(c) => c,
                None => '\\',
            }
        } else {
            c
        });
    }
    out
}

pub fn parse_frame(line: &str) -> Result<Event, String> {
    let mut fields = line.strip_prefix("@@MX\t").ok_or("Missing Maxima frame prefix")?.split('\t');
    let id = fields.next().ok_or("Missing request id")?.parse().map_err(|_| "Invalid request id")?;
    let kind = fields.next().ok_or("Missing event kind")?;
    let args: Vec<_> = fields.map(unescape).collect();
    let event = match (kind, args.as_slice()) {
        ("ready", _) => EventKind::Ready,
        ("out", [text]) => EventKind::Out(text.clone()),
        ("error", [text]) => EventKind::Error(text.clone()),
        ("plot", [text]) => EventKind::Plot(text.clone()),
        ("done", [status]) if status == "ok" || status == "err" => EventKind::Done { ok: status == "ok" },
        ("result", [label, shown, tex, text]) if shown == "0" || shown == "1" => {
            EventKind::Result(ResultItem { label: label.clone(), shown: shown == "1", tex: tex.clone(), text: text.clone() })
        }
        ("vars", args) if args.len() % 3 == 0 => {
            let mut vars = Vec::new();
            for entry in args.chunks_exact(3) {
                if entry[0] != "v" && entry[0] != "f" {
                    return Err("Invalid variable type".into());
                }
                vars.push(VarInfo { name: entry[1].clone(), value: entry[2].clone(), is_function: entry[0] == "f" });
            }
            EventKind::Vars(vars)
        }
        _ => return Err(format!("Malformed Maxima {kind} frame")),
    };
    Ok(Event { id, kind: event })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_unicode_and_escaped_output() {
        assert_eq!(parse_frame("@@MX\t2\tout\tα\\n\\t\\\\").unwrap(), Event {
            id: 2, kind: EventKind::Out("α\n\t\\".into()),
        });
    }

    #[test]
    fn rejects_incomplete_and_unknown_frames() {
        for line in ["@@MX\t1\tresult\tlabel", "@@MX\t1\tdone\tmaybe", "@@MX\t1\tvars\tv\tx", "@@MX\t1\tunknown"] {
            assert!(parse_frame(line).is_err(), "{line}");
        }
    }
}
