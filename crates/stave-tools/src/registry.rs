//! The registry: every command by id, its input checked against its
//! fields before it runs, and its fields as JSON Schema.

use serde_json::{json, Map, Value};

use crate::command::{Command, CommandError, CommandInfo, Context, Field, FnCommand, Input, Kind, Outcome, RunFn};

/// Commands by id, in registration order (the order help and tool lists
/// show).
#[derive(Default)]
pub struct Registry {
    commands: Vec<Box<dyn Command>>,
}

impl Registry {
    /// An empty registry ([`crate::registry()`] has the built-in commands).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a command; a second command with the same id replaces the
    /// first (a host app can override a built-in).
    pub fn register(&mut self, command: Box<dyn Command>) -> &mut Self {
        let id = command.info().id;
        match self.commands.iter().position(|c| c.info().id == id) {
            Some(i) => self.commands[i] = command,
            None => self.commands.push(command),
        }
        self
    }

    pub fn register_fn(&mut self, info: CommandInfo, run: RunFn) -> &mut Self {
        self.register(Box::new(FnCommand { info, run }))
    }

    pub fn get(&self, id: &str) -> Option<&dyn Command> {
        self.commands.iter().find(|c| c.info().id == id).map(|c| c.as_ref())
    }

    /// By MCP tool name (`fit_stimulus`).
    pub fn by_tool_name(&self, name: &str) -> Option<&dyn Command> {
        self.commands
            .iter()
            .find(|c| c.info().tool_name() == name)
            .map(|c| c.as_ref())
    }

    pub fn commands(&self) -> impl Iterator<Item = &dyn Command> {
        self.commands.iter().map(|c| c.as_ref())
    }

    /// Check `input` against the command's fields, then run it.
    pub fn run(&self, id: &str, input: Value, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
        let command = self
            .get(id)
            .ok_or_else(|| CommandError::Usage(format!("no command '{id}'")))?;
        let input = validate(command.info(), input)?;
        command.run(&input, cx)
    }
}

/// `input` as the command's [`Input`]: an object, no unknown keys,
/// every value of its field's kind (numbers given as strings are read,
/// as a command line gives them), required fields present, defaults
/// filled in.
pub fn validate(info: &CommandInfo, input: Value) -> Result<Input, CommandError> {
    let usage = |m: String| CommandError::Usage(format!("{}: {m}", info.id));
    let mut given = match input {
        Value::Object(m) => m,
        Value::Null => Map::new(),
        other => return Err(usage(format!("input must be an object, not {other}"))),
    };
    let mut out = Map::new();
    for f in info.inputs {
        let value = match given.remove(f.name) {
            Some(Value::Null) | None => match f.default {
                Some(d) => Some(coerce(f, Value::String(d.into())).map_err(&usage)?),
                None if f.required => return Err(usage(format!("{} is required", f.name))),
                None => None,
            },
            Some(v) => Some(coerce(f, v).map_err(&usage)?),
        };
        if let Some(v) = value {
            out.insert(f.name.into(), v);
        }
    }
    if let Some(k) = given.keys().next() {
        let known: Vec<&str> = info.inputs.iter().map(|f| f.name).collect();
        return Err(usage(format!("unknown input '{k}' (takes: {})", known.join(", "))));
    }
    Ok(Input(out))
}

/// One value as its field's kind.
fn coerce(f: &Field, v: Value) -> Result<Value, String> {
    let bad = |v: &Value| format!("{}: {v} is not {}", f.name, kind_words(f.kind));
    Ok(match (f.kind, v) {
        (Kind::String, Value::String(s)) => Value::String(s),
        (Kind::Number, Value::Number(n)) => Value::Number(n),
        (Kind::Number, Value::String(s)) => match s.trim().parse::<f64>() {
            Ok(x) if x.is_finite() => json!(x),
            _ => return Err(bad(&Value::String(s))),
        },
        (Kind::Integer, Value::Number(n)) if n.as_u64().is_some() => Value::Number(n),
        (Kind::Integer, Value::String(s)) => match s.trim().parse::<u64>() {
            Ok(x) => json!(x),
            _ => return Err(bad(&Value::String(s))),
        },
        (Kind::Bool, Value::Bool(b)) => Value::Bool(b),
        (Kind::Bool, Value::String(s)) => match s.as_str() {
            "true" | "1" | "yes" => Value::Bool(true),
            "false" | "0" | "no" => Value::Bool(false),
            _ => return Err(bad(&Value::String(s))),
        },
        (Kind::Strings, Value::String(s)) => json!([s]),
        (Kind::Strings, Value::Array(a)) if a.iter().all(Value::is_string) => Value::Array(a),
        (Kind::Choice(words), Value::String(s)) if words.contains(&s.as_str()) => Value::String(s),
        (Kind::Json, v) => v,
        (_, v) => return Err(bad(&v)),
    })
}

fn kind_words(kind: Kind) -> String {
    match kind {
        Kind::String => "a string".into(),
        Kind::Number => "a number".into(),
        Kind::Integer => "a whole number".into(),
        Kind::Bool => "true or false".into(),
        Kind::Strings => "a list of strings".into(),
        Kind::Choice(w) => format!("one of {}", w.join(", ")),
        Kind::Json => "JSON".into(),
    }
}

/// The JSON Schema of one field.
fn field_schema(f: &Field) -> Value {
    let mut s = match f.kind {
        Kind::String => json!({ "type": "string" }),
        Kind::Number => json!({ "type": "number" }),
        Kind::Integer => json!({ "type": "integer", "minimum": 0 }),
        Kind::Bool => json!({ "type": "boolean" }),
        Kind::Strings => json!({ "type": "array", "items": { "type": "string" } }),
        Kind::Choice(words) => json!({ "type": "string", "enum": words }),
        Kind::Json => json!({}),
    };
    s["description"] = Value::String(f.help.into());
    if let Some(d) = f.default {
        if let Ok(v) = coerce(f, Value::String(d.into())) {
            s["default"] = v;
        }
    }
    s
}

fn object_schema(fields: &[Field]) -> Value {
    let properties: Map<String, Value> = fields.iter().map(|f| (f.name.into(), field_schema(f))).collect();
    let required: Vec<&str> = fields
        .iter()
        .filter(|f| f.required && f.default.is_none())
        .map(|f| f.name)
        .collect();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// The command's inputs as a JSON Schema object (MCP's `inputSchema`).
pub fn input_schema(info: &CommandInfo) -> Value {
    object_schema(info.inputs)
}

/// [`crate::command::Outcome::data`]'s top-level fields as a JSON Schema
/// object (MCP's `outputSchema`). Nested reports are documented by
/// their field's help, not spelled out.
pub fn output_schema(info: &CommandInfo) -> Value {
    let mut s = object_schema(info.outputs);
    s["additionalProperties"] = Value::Bool(true);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Cancel, Status};
    use crate::target::Workspace;

    const ECHO: CommandInfo = CommandInfo {
        id: "test.echo",
        title: "Echo",
        summary: "Echo the input",
        description: "",
        inputs: &[
            Field::positional("what", Kind::String, "what to echo"),
            Field::flag("times", Kind::Integer, "how often").default("1"),
            Field::flag("loud", Kind::Bool, "shout"),
            Field::flag("tag", Kind::Strings, "tags"),
            Field::flag("mode", Kind::Choice(&["a", "b"]), "mode"),
        ],
        outputs: &[Field::flag("echo", Kind::String, "the echo").required()],
        long_running: false,
    };

    fn echo(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
        cx.progress(1.0, Some(1.0), "echoed");
        let s = input.str("what")?.repeat(input.integer("times").unwrap_or(0) as usize);
        Outcome::ok(json!({ "echo": s }), s.clone())
    }

    fn run(input: Value) -> Result<Outcome, CommandError> {
        let mut r = Registry::new();
        r.register_fn(ECHO, echo);
        let mut ws = Workspace::default();
        let mut seen = Vec::new();
        let mut sink = |p: &crate::command::Progress| seen.push(p.message.clone());
        let out = r.run("test.echo", input, &mut Context::new(&mut ws, &mut sink, Cancel::default()));
        if out.is_ok() {
            assert_eq!(seen, ["echoed"]);
        }
        out
    }

    #[test]
    fn validates_and_fills_defaults() {
        let o = run(json!({ "what": "ab" })).unwrap();
        assert_eq!((o.status, o.data["echo"].as_str()), (Status::Ok, Some("ab")));
        let o = run(json!({ "what": "ab", "times": "3" })).unwrap();
        assert_eq!(o.text, "ababab", "numbers given as strings are read");
    }

    #[test]
    fn refuses_bad_input_as_usage() {
        for (input, says) in [
            (json!({}), "what is required"),
            (json!({ "what": "x", "times": -1 }), "times: -1 is not a whole number"),
            (json!({ "what": "x", "loud": "maybe" }), "loud"),
            (json!({ "what": "x", "mode": "c" }), "one of a, b"),
            (json!({ "what": "x", "nope": 1 }), "unknown input 'nope'"),
            (json!([1]), "must be an object"),
        ] {
            match run(input.clone()) {
                Err(CommandError::Usage(m)) => assert!(m.contains(says), "{input}: {m}"),
                other => panic!("{input}: {other:?}"),
            }
        }
    }

    #[test]
    fn schemas_name_every_field() {
        let s = input_schema(&ECHO);
        assert_eq!(s["required"], json!(["what"]));
        assert_eq!(s["properties"]["times"]["default"], json!(1));
        assert_eq!(s["properties"]["mode"]["enum"], json!(["a", "b"]));
        assert_eq!(s["properties"]["tag"]["type"], "array");
        assert_eq!(output_schema(&ECHO)["required"], json!(["echo"]));
        assert_eq!(ECHO.tool_name(), "test_echo");
        assert_eq!(ECHO.words().collect::<Vec<_>>(), ["test", "echo"]);
    }

    #[test]
    fn a_later_registration_replaces() {
        let mut r = Registry::new();
        r.register_fn(ECHO, echo);
        r.register_fn(ECHO, |_, _| Outcome::ok(json!({ "echo": "over" }), "over"));
        assert_eq!(r.commands().count(), 1);
        assert!(r.by_tool_name("test_echo").is_some());
    }
}
