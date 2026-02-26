#![allow(unused)]

use std::env;
use std::fs;
use std::io::Write;

use quick_xml::de::from_str;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Protocol {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "$value")]
    items: Vec<ProtocolItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ProtocolItem {
    Copyright(Copyright),
    Interface(Interface),
    Description(Description),
}

#[derive(Debug, Deserialize)]
struct Copyright {
    #[serde(rename = "$value")]
    text: String,
}

#[derive(Debug, Deserialize)]
struct Interface {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@version")]
    version: u32,
    #[serde(rename = "$value")]
    items: Vec<InterfaceItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum InterfaceItem {
    Description(Description),
    Request(Request),
    Event(Event),
    Enum(Enum),
}

#[derive(Debug, Deserialize)]
struct Request {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@type")]
    req_type: Option<String>,
    #[serde(rename = "@since")]
    since: Option<u32>,
    #[serde(rename = "$value")]
    items: Vec<RequestItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RequestItem {
    Description(Description),
    Arg(Arg),
}

#[derive(Debug, Deserialize)]
struct Event {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@since")]
    since: Option<u32>,
    #[serde(rename = "$value")]
    items: Vec<EventItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EventItem {
    Description(Description),
    Arg(Arg),
}

#[derive(Debug, Deserialize)]
struct Enum {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@since")]
    since: Option<u32>,
    #[serde(rename = "@bitfield")]
    bitfield: Option<bool>,
    #[serde(rename = "$value")]
    items: Vec<EnumItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EnumItem {
    Description(Description),
    Entry(Entry),
}

#[derive(Debug, Deserialize)]
struct Entry {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@value")]
    value: String,
    #[serde(rename = "@summary")]
    summary: Option<String>,
    #[serde(rename = "@since")]
    since: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct Arg {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@type")]
    arg_type: String,
    #[serde(rename = "@summary")]
    summary: Option<String>,
    #[serde(rename = "@interface")]
    interface: Option<String>,
    #[serde(rename = "@allow-null")]
    allow_null: Option<bool>,
    #[serde(rename = "@enum")]
    enum_type: Option<String>,
    #[serde(rename = "description")]
    description: Option<Description>,
}

#[derive(Debug, Deserialize)]
struct Description {
    #[serde(rename = "@summary")]
    summary: Option<String>,
    #[serde(rename = "$value")]
    text: Option<String>,
}

impl Protocol {
    fn interfaces(&self) -> Vec<&Interface> {
        self.items
            .iter()
            .filter_map(|item| match item {
                ProtocolItem::Interface(i) => Some(i),
                _ => None,
            })
            .collect()
    }
}

fn open(s: &str) -> std::io::Result<std::io::BufWriter<std::fs::File>> {
    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
    let mut path = std::path::PathBuf::from(out_dir);
    path.push(s);
    Ok(std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?,
    ))
}

/// Prefix Rust keywords with `r#` so they can be used as identifiers.
fn escape_keyword(s: &str) -> String {
    match s {
        "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn"
        | "else" | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl" | "in"
        | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub" | "ref" | "return"
        | "self" | "Self" | "static" | "struct" | "super" | "trait" | "true" | "type"
        | "union" | "unsafe" | "use" | "where" | "while" => format!("r#{}", s),
        _ => s.to_string(),
    }
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect()
}

/// Rust type for an event struct field.
fn arg_rust_type(arg: &Arg) -> &str {
    match arg.arg_type.as_str() {
        "int" | "fixed" => "i32",
        "uint" | "object" | "new_id" => "u32",
        "string" => "String",
        "array" => "Vec<u8>",
        "fd" => "i32",
        _ => "u32",
    }
}

/// Rust type for a request method parameter.
fn arg_param_type(arg: &Arg) -> &str {
    match arg.arg_type.as_str() {
        "int" | "fixed" => "i32",
        "uint" => "u32",
        "object" | "new_id" => "impl crate::object::Object",
        "string" => "&str",
        "array" => "&[u8]",
        "fd" => "i32",
        _ => "u32",
    }
}

/// Encode expression for a named parameter.
fn arg_encode(arg: &Arg, param: &str) -> String {
    match arg.arg_type.as_str() {
        "uint" => {
            format!("crate::wire::write_u32(&mut args, {});", param)
        }
        "object" | "new_id" => {
            format!("crate::wire::write_u32(&mut args, {}.object_id());", param)
        }
        "int" | "fixed" => {
            format!("crate::wire::write_u32(&mut args, {} as u32);", param)
        }
        "string" => {
            format!("crate::wire::write_string(&mut args, {});", param)
        }
        "array" => format!(
            "{{ let _arr = {}; crate::wire::write_u32(&mut args, _arr.len() as u32); \
             args.extend_from_slice(_arr); \
             let _pad = (4 - (_arr.len() % 4)) % 4; for _ in 0.._pad {{ args.push(0); }} }}",
            param
        ),
        "fd" => "unimplemented!(\"fd passing not supported\");".to_string(),
        _ => format!("crate::wire::write_u32(&mut args, {});", param),
    }
}

fn request_args(req: &Request) -> Vec<&Arg> {
    req.items
        .iter()
        .filter_map(|item| match item {
            RequestItem::Arg(a) => Some(a),
            _ => None,
        })
        .collect()
}

fn event_args(evt: &Event) -> Vec<&Arg> {
    evt.items
        .iter()
        .filter_map(|item| match item {
            EventItem::Arg(a) => Some(a),
            _ => None,
        })
        .collect()
}

fn has_untyped_new_id(args: &[&Arg]) -> bool {
    args.iter()
        .any(|a| a.arg_type == "new_id" && a.interface.is_none())
}

// ── Code generators ──────────────────────────────────────────────────────────

fn emit_request_method(
    f: &mut impl Write,
    iface_name: &str,
    req: &Request,
) -> std::io::Result<()> {
    let args = request_args(req);
    let method_name = escape_keyword(&req.name.replace('-', "_"));
    let opcode_path = format!(
        "{}::request::{}",
        iface_name,
        req.name.to_ascii_uppercase().replace('-', "_")
    );

    let mut params = String::new();
    let mut encode_lines: Vec<String> = Vec::new();

    if has_untyped_new_id(&args) {
        // Expand untyped new_id into three params: interface, version, id
        for arg in &args {
            if arg.arg_type == "fd" {
                continue;
            }
            if arg.arg_type == "new_id" && arg.interface.is_none() {
                if !params.is_empty() {
                    params.push_str(", ");
                }
                params.push_str("interface: &str, version: u32, id: impl crate::object::Object");
                encode_lines.push("crate::wire::write_string(&mut args, interface);".to_string());
                encode_lines.push("crate::wire::write_u32(&mut args, version);".to_string());
                encode_lines.push("crate::wire::write_u32(&mut args, id.object_id());".to_string());
            } else {
                let pname = escape_keyword(&arg.name);
                if !params.is_empty() {
                    params.push_str(", ");
                }
                params.push_str(&format!("{}: {}", pname, arg_param_type(arg)));
                encode_lines.push(arg_encode(arg, &pname));
            }
        }
    } else {
        for arg in &args {
            if arg.arg_type == "fd" {
                continue;
            }
            let pname = escape_keyword(&arg.name);
            if !params.is_empty() {
                params.push_str(", ");
            }
            params.push_str(&format!("{}: {}", pname, arg_param_type(arg)));
            encode_lines.push(arg_encode(arg, &pname));
        }
    }

    let extra_params = if params.is_empty() {
        String::new()
    } else {
        format!(", {}", params)
    };

    writeln!(
        f,
        "    fn {}(&self, conn: &mut crate::connection::Connection{}) -> std::io::Result<()> {{",
        method_name, extra_params
    )?;
    writeln!(
        f,
        "        tracing::debug!(object_id = self.object_id(), opcode = {}, \"{}.{}\");",
        opcode_path,
        iface_name,
        req.name.replace('-', "_"),
    )?;
    if encode_lines.is_empty() {
        writeln!(f, "        let args: &[u8] = &[];")?;
    } else {
        writeln!(f, "        let mut args = Vec::new();")?;
        for line in &encode_lines {
            writeln!(f, "        {}", line)?;
        }
    }
    writeln!(
        f,
        "        conn.send_msg(self.object_id(), {}, &args)",
        opcode_path
    )?;
    writeln!(f, "    }}")?;
    writeln!(f)?;
    Ok(())
}

fn emit_dispatch(f: &mut impl Write, iface_name: &str, events: &[&Event]) -> std::io::Result<()> {
    // Decide whether body will actually be read in any arm.
    let any_args = events.iter().any(|evt| {
        event_args(evt)
            .into_iter()
            .any(|a| a.arg_type != "fd")
    });
    let body_param = if any_args { "body" } else { "_body" };

    writeln!(f, "    fn dispatch(&mut self, opcode: u16, {}: &[u8]) {{", body_param)?;

    if events.is_empty() {
        writeln!(f, "        let _ = opcode;")?;
        writeln!(f, "    }}")?;
        return Ok(());
    }

    writeln!(f, "        match opcode {{")?;

    for evt in events {
        let args = event_args(evt);
        let args_no_fd: Vec<&Arg> = args.iter().copied().filter(|a| a.arg_type != "fd").collect();
        let opcode_const = format!(
            "{}::event::{}",
            iface_name,
            evt.name.to_ascii_uppercase().replace('-', "_")
        );
        let cb_name = format!("on_{}", evt.name.replace('-', "_"));

        writeln!(f, "            {} => {{", opcode_const)?;
        writeln!(
            f,
            "                tracing::trace!(opcode = {}, \"{}.{}\");",
            opcode_const,
            iface_name,
            evt.name.replace('-', "_"),
        )?;

        if args_no_fd.is_empty() {
            writeln!(f, "                self.{}();", cb_name)?;
        } else {
            // Decode each argument in order, tracking offset as literal or variable.
            // off_var = None means use off_lit (a constant known at codegen time).
            // off_var = Some(name) means use a runtime variable.
            let mut off_var: Option<String> = None;
            let mut off_lit: usize = 0;
            let mut field_names: Vec<String> = Vec::new();

            for (i, arg) in args_no_fd.iter().enumerate() {
                let var = escape_keyword(&arg.name);
                field_names.push(var.clone());

                let cur_off = match &off_var {
                    None => off_lit.to_string(),
                    Some(v) => v.clone(),
                };

                match arg.arg_type.as_str() {
                    "uint" | "object" | "new_id" => {
                        writeln!(
                            f,
                            "                let {} = crate::wire::read_u32(body, {});",
                            var, cur_off
                        )?;
                        if off_var.is_none() {
                            off_lit += 4;
                        } else {
                            let nv = format!("_off{}", i);
                            writeln!(
                                f,
                                "                let {} = {} + 4;",
                                nv,
                                off_var.as_ref().unwrap()
                            )?;
                            off_var = Some(nv);
                        }
                    }
                    "int" | "fixed" => {
                        writeln!(
                            f,
                            "                let {} = crate::wire::read_u32(body, {}) as i32;",
                            var, cur_off
                        )?;
                        if off_var.is_none() {
                            off_lit += 4;
                        } else {
                            let nv = format!("_off{}", i);
                            writeln!(
                                f,
                                "                let {} = {} + 4;",
                                nv,
                                off_var.as_ref().unwrap()
                            )?;
                            off_var = Some(nv);
                        }
                    }
                    "string" => {
                        let nv = format!("_off{}", i);
                        writeln!(
                            f,
                            "                let ({}, {}) = crate::wire::read_string(body, {});",
                            var, nv, cur_off
                        )?;
                        off_var = Some(nv);
                    }
                    "array" => {
                        let len_var = format!("_arr_len{}", i);
                        let nv = format!("_off{}", i);
                        writeln!(
                            f,
                            "                let {} = crate::wire::read_u32(body, {}) as usize;",
                            len_var, cur_off
                        )?;
                        writeln!(
                            f,
                            "                let {} = body[{} + 4..{} + 4 + {}].to_vec();",
                            var, cur_off, cur_off, len_var
                        )?;
                        writeln!(
                            f,
                            "                let {} = {} + 4 + {} + (4 - ({} % 4)) % 4;",
                            nv, cur_off, len_var, len_var
                        )?;
                        off_var = Some(nv);
                    }
                    _ => {
                        writeln!(
                            f,
                            "                let {} = crate::wire::read_u32(body, {});",
                            var, cur_off
                        )?;
                        if off_var.is_none() {
                            off_lit += 4;
                        } else {
                            let nv = format!("_off{}", i);
                            writeln!(
                                f,
                                "                let {} = {} + 4;",
                                nv,
                                off_var.as_ref().unwrap()
                            )?;
                            off_var = Some(nv);
                        }
                    }
                }
            }

            let iface_pascal = to_pascal_case(iface_name);
            let struct_name = format!("{}{}Event", iface_pascal, to_pascal_case(&evt.name));
            let fields = field_names.join(", ");
            writeln!(
                f,
                "                self.{}({} {{ {} }});",
                cb_name, struct_name, fields
            )?;
        }

        writeln!(f, "            }}")?;
    }

    writeln!(f, "            _ => {{}}")?;
    writeln!(f, "        }}")?;
    writeln!(f, "    }}")?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let filename = "wayland.xml";
    println!("cargo:rerun-if-changed={}", filename);

    let xml_content = fs::read_to_string(filename)
        .map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", filename, e))?;
    let protocol: Protocol = from_str(&xml_content)?;

    let mut f = open("wayland_protocol.rs")?;
    writeln!(f, "// Auto-generated from {}. DO NOT EDIT.", filename)?;
    writeln!(f)?;

    // ── Pass 1: per-interface constant modules ────────────────────────────────
    for iface in protocol.interfaces() {
        let requests: Vec<&Request> = iface
            .items
            .iter()
            .filter_map(|item| match item {
                InterfaceItem::Request(r) => Some(r),
                _ => None,
            })
            .collect();

        let events: Vec<&Event> = iface
            .items
            .iter()
            .filter_map(|item| match item {
                InterfaceItem::Event(e) => Some(e),
                _ => None,
            })
            .collect();

        writeln!(f, "pub mod {} {{", iface.name)?;
        writeln!(f, "    pub const INTERFACE: &str = {:?};", iface.name)?;
        writeln!(f, "    pub const VERSION: u32 = {};", iface.version)?;

        if !requests.is_empty() {
            writeln!(f, "    pub mod request {{")?;
            for (opcode, req) in requests.iter().enumerate() {
                let const_name = req.name.to_ascii_uppercase().replace('-', "_");
                writeln!(f, "        pub const {}: u16 = {};", const_name, opcode)?;
            }
            writeln!(f, "    }}")?;
        }

        if !events.is_empty() {
            writeln!(f, "    pub mod event {{")?;
            for (opcode, evt) in events.iter().enumerate() {
                let const_name = evt.name.to_ascii_uppercase().replace('-', "_");
                writeln!(f, "        pub const {}: u16 = {};", const_name, opcode)?;
            }
            writeln!(f, "    }}")?;
        }

        writeln!(f, "}}")?;
        writeln!(f)?;
    }

    // ── Pass 2: event structs + handler traits ────────────────────────────────
    for iface in protocol.interfaces() {
        let iface_pascal = to_pascal_case(&iface.name);

        let requests: Vec<&Request> = iface
            .items
            .iter()
            .filter_map(|item| match item {
                InterfaceItem::Request(r) => Some(r),
                _ => None,
            })
            .collect();

        let events: Vec<&Event> = iface
            .items
            .iter()
            .filter_map(|item| match item {
                InterfaceItem::Event(e) => Some(e),
                _ => None,
            })
            .collect();

        // Event structs (only for events with ≥1 non-fd arg)
        for evt in &events {
            let args: Vec<&Arg> = event_args(evt)
                .into_iter()
                .filter(|a| a.arg_type != "fd")
                .collect();
            if args.is_empty() {
                continue;
            }
            let struct_name = format!("{}{}Event", iface_pascal, to_pascal_case(&evt.name));
            writeln!(f, "pub struct {} {{", struct_name)?;
            for arg in &args {
                writeln!(
                    f,
                    "    pub {}: {},",
                    escape_keyword(&arg.name),
                    arg_rust_type(arg)
                )?;
            }
            writeln!(f, "}}")?;
            writeln!(f)?;
        }

        // Concrete struct + Object impl
        writeln!(f, "pub struct {} {{", iface_pascal)?;
        writeln!(f, "    object_id: u32,")?;
        writeln!(f, "}}")?;
        writeln!(f)?;
        writeln!(f, "impl {} {{", iface_pascal)?;
        writeln!(f, "    pub fn new(object_id: u32) -> Self {{")?;
        writeln!(f, "        {} {{ object_id }}", iface_pascal)?;
        writeln!(f, "    }}")?;
        writeln!(f, "}}")?;
        writeln!(f)?;
        writeln!(f, "impl crate::object::Object for {} {{", iface_pascal)?;
        writeln!(f, "    fn object_id(&self) -> u32 {{ self.object_id }}")?;
        writeln!(f, "}}")?;
        writeln!(f)?;

        // Handler trait
        let trait_name = format!("{}Handler", iface_pascal);
        writeln!(f, "pub trait {}: crate::object::Object {{", trait_name)?;
        writeln!(f)?;

        // Request methods (default impls)
        for req in &requests {
            emit_request_method(&mut f, &iface.name, req)?;
        }

        // Event callbacks (empty default impls)
        for evt in &events {
            let args: Vec<&Arg> = event_args(evt)
                .into_iter()
                .filter(|a| a.arg_type != "fd")
                .collect();
            let cb_name = format!("on_{}", evt.name.replace('-', "_"));
            if args.is_empty() {
                writeln!(f, "    fn {}(&mut self) {{}}", cb_name)?;
            } else {
                let struct_name = format!("{}{}Event", iface_pascal, to_pascal_case(&evt.name));
                writeln!(
                    f,
                    "    fn {}(&mut self, _event: {}) {{}}",
                    cb_name, struct_name
                )?;
            }
        }

        if !events.is_empty() {
            writeln!(f)?;
        }

        // Dispatch method
        let events_refs: Vec<&Event> = events.iter().copied().collect();
        emit_dispatch(&mut f, &iface.name, &events_refs)?;

        writeln!(f, "}}")?;
        writeln!(f)?;
    }

    Ok(())
}
