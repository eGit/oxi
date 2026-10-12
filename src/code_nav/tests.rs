use super::*;

fn definition(
    path: &str,
    source: &str,
    at: usize,
    buffers: &[(&str, &str)],
) -> Option<(PathBuf, String)> {
    let request = DefinitionRequest {
        current_path: PathBuf::from(path),
        current_source: source.to_owned(),
        cursor_byte: at,
        workspace_files: Vec::new(),
        open_buffers: buffers
            .iter()
            .map(|(path, text)| (PathBuf::from(path), (*text).to_owned()))
            .collect(),
    };
    let location = find_definition(&SymbolIndex::default(), &request)?;
    let text = if location.path == request.current_path {
        source.to_owned()
    } else {
        buffers
            .iter()
            .find(|(path, _)| Path::new(path) == location.path)?
            .1
            .to_owned()
    };
    // Return the line of the definition so assertions read naturally.
    let line_start = text[..location.byte_range.start]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let line_end = text[location.byte_range.start..]
        .find('\n')
        .map_or(text.len(), |i| location.byte_range.start + i);
    Some((location.path, text[line_start..line_end].trim().to_owned()))
}

#[test]
fn every_language_query_compiles() {
    for language in [
        "rs", "py", "js", "jsx", "ts", "tsx", "go", "c", "cpp", "java", "sh",
    ] {
        assert!(supports(language), "{language} query failed to compile");
    }
}

#[test]
fn identifier_works_at_middle_and_end() {
    assert_eq!(identifier_at("let hello = 1", 6), Some(("hello", 4..9)));
    assert_eq!(identifier_at("hello", 5), Some(("hello", 0..5)));
    assert_eq!(identifier_at(" ", 1), None);
}

#[test]
fn rust_local_shadows_function() {
    let source = "fn helper() {}\nfn main() { let helper = 3; dbg!(helper); }\n";
    let at = source.rfind("helper").unwrap();
    let (_, line) = definition("/w/main.rs", source, at, &[]).unwrap();
    assert!(line.contains("let helper = 3"), "{line}");
}

#[test]
fn rust_local_out_of_scope_falls_back_to_item() {
    let source = "fn helper() {}\nfn a() { let helper = 1; }\nfn b() { helper(); }\n";
    let at = source.rfind("helper").unwrap();
    let (_, line) = definition("/w/main.rs", source, at, &[]).unwrap();
    assert_eq!(line, "fn helper() {}");
}

#[test]
fn rust_pattern_bindings_skip_variants() {
    let symbols = file_symbols(
        "rs",
        "fn f(o: Option<u8>) { if let Some(value) = o { value; } }",
    );
    let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"value"));
    assert!(!names.contains(&"Some"));
    assert!(names.contains(&"o"));
}

#[test]
fn qualifier_prefers_matching_module_file() {
    let source = "fn main() { util::run(); }";
    let buffers = [
        ("/w/util.rs", "pub fn run() {}"),
        ("/w/other.rs", "pub fn run() {}"),
    ];
    let at = source.find("run").unwrap();
    let (path, _) = definition("/w/main.rs", source, at, &buffers).unwrap();
    assert_eq!(path, PathBuf::from("/w/util.rs"));
}

#[test]
fn python_parameters_methods_and_fields() {
    let source = "class Stats:\n    def __init__(self, values):\n        self.values = values\n\n    def mean(self):\n        return sum(self.values)\n\ndef run(values):\n    return Stats(values).mean()\n";
    let at = source.rfind("values").unwrap();
    let (_, line) = definition("/w/stats.py", source, at, &[]).unwrap();
    assert_eq!(line, "def run(values):");
    let at = source.rfind("mean").unwrap();
    let (_, line) = definition("/w/stats.py", source, at, &[]).unwrap();
    assert_eq!(line, "def mean(self):");
    let at = source.find("sum(self.values)").unwrap() + "sum(self.".len();
    let (_, line) = definition("/w/stats.py", source, at, &[]).unwrap();
    assert_eq!(line, "self.values = values");
}

#[test]
fn python_cross_file_definition_beats_import() {
    let source = "from helpers import median\n\nprint(median([1]))\n";
    let at = source.rfind("median").unwrap();
    let buffers = [("/w/helpers.py", "def median(values):\n    pass\n")];
    let (path, line) = definition("/w/main.py", source, at, &buffers).unwrap();
    assert_eq!(path, PathBuf::from("/w/helpers.py"));
    assert_eq!(line, "def median(values):");
    // Without the defining file, the import line is the best answer.
    let (path, _) = definition("/w/main.py", source, at, &[]).unwrap();
    assert_eq!(path, PathBuf::from("/w/main.py"));
}

#[test]
fn typescript_and_javascript_definitions() {
    let source = "interface Point { x: number }\nconst dist = (p: Point) => p.x;\nfunction main() { const p = { x: 1 }; return dist(p); }\n";
    let at = source.rfind("dist").unwrap();
    let (_, line) = definition("/w/a.ts", source, at, &[]).unwrap();
    assert!(line.starts_with("const dist"), "{line}");
    let at = source.find("(p: Point)").unwrap() + 5;
    let (_, line) = definition("/w/a.ts", source, at, &[]).unwrap();
    assert!(line.starts_with("interface Point"), "{line}");
    let at = source.rfind("(p)").unwrap() + 1;
    let (_, line) = definition("/w/a.ts", source, at, &[]).unwrap();
    assert!(line.starts_with("function main"), "{line}");

    let js = "import { util } from './u.js';\nclass A { run() {} }\nnew A().run(util);\n";
    let at = js.rfind("run").unwrap();
    let (_, line) = definition("/w/a.js", js, at, &[]).unwrap();
    assert_eq!(line, "class A { run() {} }");
    let buffers = [("/w/u.ts", "export function util() {}\n")];
    let at = js.rfind("util").unwrap();
    let (path, _) = definition("/w/a.js", js, at, &buffers).unwrap();
    assert_eq!(path, PathBuf::from("/w/u.ts"));
}

#[test]
fn go_c_java_definitions() {
    let go = "package main\ntype Server struct { port int }\nfunc (s *Server) Run() {}\nfunc main() { srv := &Server{}; srv.Run() }\n";
    let at = go.rfind("Run").unwrap();
    let (_, line) = definition("/w/main.go", go, at, &[]).unwrap();
    assert_eq!(line, "func (s *Server) Run() {}");
    let at = go.rfind("srv").unwrap();
    let (_, line) = definition("/w/main.go", go, at, &[]).unwrap();
    assert!(line.contains("srv := &Server{}"), "{line}");

    let c = "int add(int a, int b);\nint add(int a, int b) { return a + b; }\nint main(void) { return add(1, 2); }\n";
    let at = c.rfind("add").unwrap();
    let (_, line) = definition("/w/main.c", c, at, &[]).unwrap();
    assert!(line.contains("return a + b"), "{line}");

    let java = "class App {\n  int count;\n  void run(int times) { count += times; }\n}\n";
    let at = java.rfind("times").unwrap();
    let (_, line) = definition("/w/App.java", java, at, &[]).unwrap();
    assert!(line.starts_with("void run"), "{line}");
    let at = java.rfind("count").unwrap();
    let (_, line) = definition("/w/App.java", java, at, &[]).unwrap();
    assert_eq!(line, "int count;");
}

#[test]
fn outline_lists_functions_and_types_only() {
    let symbols = file_symbols(
        "py",
        "X = 1\nclass A:\n    def f(self, y):\n        z = y\n",
    );
    let outline: Vec<_> = symbols
        .iter()
        .filter(|symbol| symbol.kind.is_outline())
        .map(|symbol| symbol.name.as_str())
        .collect();
    assert_eq!(outline, ["A", "f"]);
}

#[test]
fn expand_selection_grows_through_syntax_nodes() {
    let source = "fn main() { let total = add(1, 2); }";
    let caret = source.find("add").unwrap() + 1;
    let first = expand_selection("rs", source, caret..caret).unwrap();
    assert_eq!(&source[first.clone()], "add");
    let second = expand_selection("rs", source, first).unwrap();
    assert_eq!(&source[second], "add(1, 2)");
}
