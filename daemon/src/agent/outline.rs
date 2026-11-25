//! `outline`: what a source file defines and on which line, so the model can read the part it
//! needs instead of the whole file.
//!
//! This is not a parser. Each language has a few regular expressions that are tried on the
//! lines at the top level of a file, and one for the functions one level in (the methods of a
//! class or an `impl`). A definition inside a comment or a long string can show up, and an
//! unusual layout can hide one; for finding the right lines to read that is good enough.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use super::files::{self, Folder};

/// Entries of one file when a whole folder is outlined.
const PER_FILE_IN_FOLDER: usize = 12;
const MAX_FOLDER_FILES: usize = 40;
const MAX_ENTRY_CHARS: usize = 110;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lang {
    Rust,
    Python,
    Script,
    Go,
    Swift,
    Kotlin,
    Java,
    C,
    Svelte,
}

pub const KNOWN: &str = "Rust, Python, JavaScript, TypeScript, Go, Swift, Kotlin, Java, C, C++ and Svelte";

pub fn lang_of(path: &Path) -> Option<Lang> {
    Some(match path.extension()?.to_str()?.to_lowercase().as_str() {
        "rs" => Lang::Rust,
        "py" | "pyi" => Lang::Python,
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" => Lang::Script,
        "go" => Lang::Go,
        "swift" => Lang::Swift,
        "kt" | "kts" => Lang::Kotlin,
        "java" => Lang::Java,
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "m" | "mm" => Lang::C,
        "svelte" => Lang::Svelte,
        _ => return None,
    })
}

/// The expressions of one language: `top` for lines without indentation, `member` for lines one
/// level in. Both are tried on the line without its indentation.
struct Patterns {
    top: Vec<Regex>,
    member: Vec<Regex>,
}

fn build(top: &[&str], member: &[&str]) -> Patterns {
    let all = |list: &[&str]| list.iter().map(|p| Regex::new(p).expect("outline pattern")).collect();
    Patterns { top: all(top), member: all(member) }
}

const SWIFT_WORDS: &str = r"(?:(?:public|private|fileprivate|internal|open|final|static|class|override|mutating|nonmutating|nonisolated|indirect|convenience|required|lazy|weak|dynamic|private\(set\)|@\w+(?:\([^)]*\))?)\s+)*";
const KOTLIN_WORDS: &str = r"(?:(?:public|private|internal|protected|open|abstract|final|sealed|data|enum|inline|value|annotation|suspend|override|const|lateinit|companion|expect|actual|tailrec|operator|infix|external|inner|fun|@\w+(?:\([^)]*\))?)\s+)*";
const JAVA_WORDS: &str =
    r"(?:(?:public|private|protected|static|final|abstract|sealed|non-sealed|strictfp|synchronized|native|default|@\w+(?:\([^)]*\))?)\s+)*";

fn patterns(lang: Lang) -> &'static Patterns {
    static ALL: OnceLock<Vec<(Lang, Patterns)>> = OnceLock::new();
    let all = ALL.get_or_init(|| {
        let script_top = [
            r"^(?:export\s+)?(?:default\s+)?(?:async\s+)?function\b",
            r"^(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+\w+",
            r"^(?:export\s+)?(?:declare\s+)?(?:interface|type|enum|namespace|module)\s+['\x22\w]",
            r"^export\s+(?:declare\s+)?(?:const|let|var)\s+\w+",
            r"^(?:const|let|var)\s+\w+\s*(?::[^=]+)?=\s*(?:async\s+)?(?:function\b|\([^)]*\)\s*(?::[^=]+)?=>|\w+\s*=>)",
            r"^export\s+default\b",
            r"^(?:module\.)?exports(?:\.\w+)?\s*=",
        ];
        let script_member = [r"^(?:(?:public|private|protected|static|async|override|readonly|get|set)\s+)*[#\w$]+\s*(?:<[^>]*>)?\([^)]*\)\s*(?::\s*[^{;]+)?\{\s*$"];
        let svelte_top: Vec<&str> = script_top
            .iter()
            .copied()
            .chain([r"^(?:const|let)\s+(?:\w+|\{[^}]*\})\s*(?::[^=]+)?=\s*\$(?:state|derived|props|bindable)\b", r"^\{#snippet\s+\w+", r"^<(?:script|style)\b"])
            .collect();
        vec![
            (
                Lang::Rust,
                build(
                    &[
                        r#"^(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|const|unsafe|default|extern(?:\s+"[^"]*")?)\s+)*(?:fn|struct|enum|trait|type|mod|union)\s+\w+"#,
                        r"^(?:unsafe\s+)?impl\b",
                        r"^(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+(?:mut\s+)?\w+\s*:",
                        r"^macro_rules!\s*\w+",
                    ],
                    &[r#"^(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|const|unsafe|default|extern(?:\s+"[^"]*")?)\s+)*fn\s+\w+"#],
                ),
            ),
            (Lang::Python, build(&[r"^(?:async\s+)?def\s+\w+", r"^class\s+\w+", r"^[A-Z][A-Z0-9_]*\s*(?::[^=]+)?=[^=]"], &[r"^(?:async\s+)?def\s+\w+"])),
            (Lang::Script, build(&script_top, &script_member)),
            (Lang::Go, build(&[r"^func\s+(?:\([^)]*\)\s*)?\w+", r"^type\s+\w+", r"^(?:const|var)\s+\w+"], &[])),
            (
                Lang::Swift,
                build(
                    &[
                        &format!(r"^{SWIFT_WORDS}(?:func|class|struct|enum|protocol|extension|actor|typealias|init|macro)\b"),
                        &format!(r"^{SWIFT_WORDS}(?:let|var)\s+\w+"),
                    ],
                    &[&format!(r"^{SWIFT_WORDS}(?:func\s+\S+|init[?!]?\s*[(<]|deinit\b)")],
                ),
            ),
            (
                Lang::Kotlin,
                build(
                    &[
                        &format!(r"^{KOTLIN_WORDS}(?:fun\s+\S+|class\s+\w+|interface\s+\w+|object\s+\w+|typealias\s+\w+)"),
                        &format!(r"^{KOTLIN_WORDS}(?:val|var)\s+\w+"),
                    ],
                    &[&format!(r"^{KOTLIN_WORDS}fun\s+\S+")],
                ),
            ),
            (
                Lang::Java,
                build(
                    &[&format!(r"^{JAVA_WORDS}(?:class|interface|enum|record|@interface)\s+\w+")],
                    &[
                        &format!(r"^{JAVA_WORDS}(?:class|interface|enum|record)\s+\w+"),
                        &format!(r"^{JAVA_WORDS}(?:<[^>]+>\s+)?[\w.]+(?:<[^()]*>)?(?:\[\])*\s+\w+\s*\([^;]*$"),
                        &format!(r"^{JAVA_WORDS}[A-Z]\w*\s*\([^;]*$"),
                    ],
                ),
            ),
            (
                Lang::C,
                build(
                    &[
                        r"^(?:typedef\s+)?(?:struct|class|union|enum(?:\s+class)?)\s+\w+\s*(?:[:{].*)?$",
                        r"^namespace\s+\w+",
                        r"^typedef\b.*\b\w+\s*;",
                        r"^#\s*define\s+\w+",
                        r"^using\s+\w+\s*=",
                        r"^[A-Za-z_][\w:<>,\s*&~]*?[\s*&]~?[\w:]+\s*\([^;]*$",
                        r"^[-+]\s*\([^)]*\)\s*\w+",
                        r"^@(?:interface|implementation|protocol)\s+\w+",
                    ],
                    &[],
                ),
            ),
            (Lang::Svelte, build(&svelte_top, &script_member)),
        ]
    });
    &all.iter().find(|(l, _)| *l == lang).expect("every language has patterns").1
}

/// Words that start a statement, not a definition. The loose expressions (a C function, a
/// method in a class) would take them for one.
const STATEMENTS: &[&str] = &[
    "if",
    "else",
    "for",
    "while",
    "switch",
    "case",
    "return",
    "do",
    "catch",
    "try",
    "throw",
    "new",
    "await",
    "yield",
    "delete",
    "typeof",
    "sizeof",
    "goto",
    "super",
    "this",
    "assert",
    "synchronized",
    "function",
];

/// One definition: its line (from 1), whether it is one level in, and the line as written.
#[derive(Debug, PartialEq)]
pub struct Entry {
    pub line: usize,
    pub member: bool,
    pub text: String,
}

/// How far a line is indented: 0 for none, 1 for one tab or up to four spaces, 2 beyond.
fn level(line: &str) -> usize {
    let width: usize = line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum();
    match width {
        0 => 0,
        1..=4 => 1,
        _ => 2,
    }
}

/// The definitions of a source text.
pub fn entries(lang: Lang, text: &str) -> Vec<Entry> {
    let patterns = patterns(lang);
    let mut out = Vec::new();
    // In a Svelte file the code inside <script> is often indented one level as a whole.
    let mut in_script = false;
    let mut shift = 0;
    for (i, raw) in text.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut depth = level(raw);
        if lang == Lang::Svelte {
            if trimmed.starts_with("<script") {
                in_script = true;
                shift = usize::MAX;
            } else if trimmed.starts_with("</script") {
                in_script = false;
            } else if in_script {
                if shift == usize::MAX {
                    shift = depth.min(1);
                }
                depth = depth.saturating_sub(shift);
            } else if !trimmed.starts_with("{#snippet") && !trimmed.starts_with("<style") {
                continue;
            } else {
                depth = 0;
            }
        }
        let list = match depth {
            0 => &patterns.top,
            1 => &patterns.member,
            _ => continue,
        };
        let first_word = trimmed.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
        let loose = matches!(lang, Lang::C | Lang::Java) || depth == 1;
        if loose && STATEMENTS.contains(&first_word) {
            continue;
        }
        if list.iter().any(|p| p.is_match(trimmed)) {
            let shown = trimmed.trim_end_matches(['{', ' ', '\t']).trim_end_matches(" =>");
            let mut shown: String = shown.chars().take(MAX_ENTRY_CHARS).collect();
            if shown.chars().count() == MAX_ENTRY_CHARS {
                shown.push('…');
            }
            out.push(Entry { line: i + 1, member: depth == 1, text: shown });
        }
    }
    out
}

fn render(entries: &[Entry], members: bool) -> String {
    entries
        .iter()
        .filter(|e| members || !e.member)
        .map(|e| format!("{}{}: {}", if e.member { "  " } else { "" }, e.line, e.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn of_file(folder: &Folder, file: &Path, shown: &str) -> Result<String, String> {
    let Some(lang) = lang_of(file) else {
        return Err(format!("outline does not know this kind of file. It knows {KNOWN}. Use read_file for {shown}."));
    };
    let text = files::read_text(file, shown, files::MAX_READ_BYTES)?;
    let found = entries(lang, &text);
    let head = format!("{} ({} lines)", folder.relative(file), text.lines().count());
    if found.is_empty() {
        return Ok(format!("{head}\nNo definitions were found. Use read_file to see it."));
    }
    let mut body = render(&found, true);
    if body.chars().count() > files::RESULT_CHARS {
        body = render(&found, false);
    }
    Ok(format!("{head}\n{}\nRead a part with read_file and from_line.", files::cap(&body, files::RESULT_CHARS)))
}

/// `outline`: the definitions of one source file, or of each source file in a folder.
pub fn outline(folder: &Folder, path: &str) -> Result<String, String> {
    let start = folder.existing(path)?;
    if !start.is_dir() {
        return of_file(folder, &start, path);
    }
    let mut parts: Vec<String> = Vec::new();
    let mut more = false;
    for file in files::walk(&start, Some(3), false).filter(|e| e.file_type().is_some_and(|t| t.is_file())) {
        let Some(lang) = lang_of(file.path()) else { continue };
        if parts.len() >= MAX_FOLDER_FILES {
            more = true;
            break;
        }
        let shown = folder.relative(file.path());
        let Ok(text) = files::read_text(file.path(), &shown, files::MAX_READ_BYTES) else { continue };
        let found: Vec<Entry> = entries(lang, &text).into_iter().filter(|e| !e.member).collect();
        if found.is_empty() {
            continue;
        }
        let mut part =
            format!("{shown} ({} lines)\n{}", text.lines().count(), render(&found[..found.len().min(PER_FILE_IN_FOLDER)], false));
        if found.len() > PER_FILE_IN_FOLDER {
            part.push_str(&format!("\n[{} more: outline this file to see them]", found.len() - PER_FILE_IN_FOLDER));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Ok(format!("No source files with definitions were found in {}. outline knows {KNOWN}.", folder.relative(&start)));
    }
    let mut out = parts.join("\n\n");
    if more {
        out.push_str(&format!("\n\n[truncated: only the first {MAX_FOLDER_FILES} files are shown; outline a subfolder to see more]"));
    }
    Ok(files::cap(&out, files::RESULT_CHARS))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The outline of a text as "line: text" lines, members with two spaces in front.
    fn of(lang: Lang, text: &str) -> Vec<String> {
        entries(lang, text).iter().map(|e| format!("{}{}: {}", if e.member { "  " } else { "" }, e.line, e.text)).collect()
    }

    #[test]
    fn rust() {
        let text = "use std::fmt;\n\nconst LIMIT: usize = 4;\npub static mut COUNT: u32 = 0;\n\n#[derive(Debug)]\npub struct Folder {\n    root: PathBuf,\n}\n\nimpl Folder {\n    pub fn open(path: &str) -> Result<Folder, String> {\n        let x = 1;\n        if x > 0 {\n        }\n    }\n\n    async fn later(&self) {}\n}\n\npub(crate) async fn run(folder: &Folder) -> Ran {\n}\n\nenum Mode { Ask }\ntrait Named {\n    fn name(&self) -> String;\n}\nimpl<T: Named> fmt::Display for Wrapper<T> {\n}\nmod tests {\n}\nmacro_rules! hello {\n}\ntype Shared = Arc<App>;\npub unsafe extern \"C\" fn raw() {}\nlet not_this = 1;\n";
        assert_eq!(
            of(Lang::Rust, text),
            [
                "3: const LIMIT: usize = 4;",
                "4: pub static mut COUNT: u32 = 0;",
                "7: pub struct Folder",
                "11: impl Folder",
                "  12: pub fn open(path: &str) -> Result<Folder, String>",
                "  18: async fn later(&self) {}",
                "21: pub(crate) async fn run(folder: &Folder) -> Ran",
                "24: enum Mode { Ask }",
                "25: trait Named",
                "  26: fn name(&self) -> String;",
                "28: impl<T: Named> fmt::Display for Wrapper<T>",
                "30: mod tests",
                "32: macro_rules! hello",
                "34: type Shared = Arc<App>;",
                "35: pub unsafe extern \"C\" fn raw() {}",
            ]
        );
    }

    #[test]
    fn python() {
        let text = "import os\n\nMAX_SIZE = 10\nTIMEOUT: float = 2.5\nname = 'x'\n\nclass Shop(Base):\n    tax = 0.2\n\n    def total(self, items):\n        for item in items:\n            pass\n\n    async def load(self):\n        pass\n\ndef main():\n    return 1\n\nasync def serve(port: int) -> None:\n    pass\n\nif MAX_SIZE == 10:\n    pass\n";
        assert_eq!(
            of(Lang::Python, text),
            [
                "3: MAX_SIZE = 10",
                "4: TIMEOUT: float = 2.5",
                "7: class Shop(Base):",
                "  10: def total(self, items):",
                "  14: async def load(self):",
                "17: def main():",
                "20: async def serve(port: int) -> None:",
            ]
        );
    }

    #[test]
    fn javascript_and_typescript() {
        let text = "import x from 'y';\n\nexport const LIMIT = 4;\nconst local = 3;\nconst add = (a, b) => a + b;\nconst twice = async x => x * 2;\nconst typed = (a: number): number => {\n  return a;\n};\nexport default function App() {\n  if (x) {\n  }\n}\nexport async function load(id: string): Promise<Item> {\n}\nfunction* walk() {}\nexport class Code extends Base {\n  folder = '';\n  constructor(a) {\n  }\n  async send(text: string): Promise<boolean> {\n    for (const a of b) {\n    }\n  }\n  static #make() {\n  }\n  get waiting() {\n  }\n}\nexport interface Task {\n  id: string;\n}\nexport type Mode = 'ask' | 'auto';\nenum Color { Red }\ndeclare module 'x' {\n}\nmodule.exports = { add };\nconst old = function () {};\nadd(1, 2);\n";
        assert_eq!(
            of(Lang::Script, text),
            [
                "3: export const LIMIT = 4;",
                "5: const add = (a, b) => a + b;",
                "6: const twice = async x => x * 2;",
                "7: const typed = (a: number): number",
                "10: export default function App()",
                "14: export async function load(id: string): Promise<Item>",
                "16: function* walk() {}",
                "17: export class Code extends Base",
                "  19: constructor(a)",
                "  21: async send(text: string): Promise<boolean>",
                "  25: static #make()",
                "  27: get waiting()",
                "30: export interface Task",
                "33: export type Mode = 'ask' | 'auto';",
                "34: enum Color { Red }",
                "35: declare module 'x'",
                "37: module.exports = { add };",
                "38: const old = function () {};",
            ]
        );
    }

    #[test]
    fn go() {
        let text = "package main\n\nimport \"fmt\"\n\nconst Limit = 4\nvar count int\n\ntype Shop struct {\n\tname string\n}\n\ntype Named interface {\n\tName() string\n}\n\nfunc (s *Shop) Total(items []Item) float64 {\n\tif len(items) == 0 {\n\t}\n\treturn 0\n}\n\nfunc main() {\n\tfmt.Println(1)\n}\n";
        assert_eq!(
            of(Lang::Go, text),
            [
                "5: const Limit = 4",
                "6: var count int",
                "8: type Shop struct",
                "12: type Named interface",
                "16: func (s *Shop) Total(items []Item) float64",
                "22: func main()"
            ]
        );
    }

    #[test]
    fn swift() {
        let text = "import SwiftUI\n\nlet limit = 4\nprivate var cache: [String: Int] = [:]\n\n@MainActor\nfinal class Store: ObservableObject {\n    @Published var items: [Item] = []\n\n    init(items: [Item]) {\n        self.items = items\n    }\n\n    private func load() async throws -> [Item] {\n        if items.isEmpty {\n        }\n    }\n\n    static func make() -> Store { Store(items: []) }\n}\n\npublic struct ContentView: View {\n    var body: some View {\n    }\n}\n\nenum Mode { case ask }\nprotocol Named {\n    func name() -> String\n}\nextension Store: Named {\n}\nactor Counter {}\ntypealias Done = () -> Void\nfunc main() {\n}\n@available(iOS 17, *) public final class New {}\n";
        assert_eq!(
            of(Lang::Swift, text),
            [
                "3: let limit = 4",
                "4: private var cache: [String: Int] = [:]",
                "7: final class Store: ObservableObject",
                "  10: init(items: [Item])",
                "  14: private func load() async throws -> [Item]",
                "  19: static func make() -> Store { Store(items: []) }",
                "22: public struct ContentView: View",
                "27: enum Mode { case ask }",
                "28: protocol Named",
                "  29: func name() -> String",
                "31: extension Store: Named",
                "33: actor Counter {}",
                "34: typealias Done = () -> Void",
                "35: func main()",
                "37: @available(iOS 17, *) public final class New {}",
            ]
        );
    }

    #[test]
    fn kotlin() {
        let text = "package app\n\nconst val LIMIT = 4\nprivate val cache = mutableMapOf<String, Int>()\n\ndata class Item(val name: String)\n\nsealed interface State\n\nobject Registry {\n    fun find(name: String): Item? {\n        if (name.isEmpty()) {\n        }\n        return null\n    }\n}\n\nclass Shop(private val tax: Double) : Base() {\n    override suspend fun total(items: List<Item>): Double = 0.0\n\n    companion object {\n    }\n}\n\nfun main() {\n}\n\ninternal suspend fun <T> load(id: String): T = TODO()\nfun String.shout() = uppercase()\ntypealias Done = () -> Unit\nenum class Mode { ASK }\nfun interface Named {\n}\n@Composable\nfun Screen() {\n}\n";
        assert_eq!(
            of(Lang::Kotlin, text),
            [
                "3: const val LIMIT = 4",
                "4: private val cache = mutableMapOf<String, Int>()",
                "6: data class Item(val name: String)",
                "8: sealed interface State",
                "10: object Registry",
                "  11: fun find(name: String): Item?",
                "18: class Shop(private val tax: Double) : Base()",
                "  19: override suspend fun total(items: List<Item>): Double = 0.0",
                "25: fun main()",
                "28: internal suspend fun <T> load(id: String): T = TODO()",
                "29: fun String.shout() = uppercase()",
                "30: typealias Done = () -> Unit",
                "31: enum class Mode { ASK }",
                "32: fun interface Named",
                "35: fun Screen()",
            ]
        );
    }

    #[test]
    fn java() {
        let text = "package app;\n\nimport java.util.List;\n\npublic final class Shop extends Base implements Named {\n    private static final int LIMIT = 4;\n    private final List<Item> items;\n\n    public Shop(List<Item> items) {\n        this.items = items;\n    }\n\n    @Override\n    public double total(List<Item> items) {\n        if (items.isEmpty()) {\n        }\n        return compute(items);\n    }\n\n    static <T> List<T> wrap(T one) {\n        return List.of(one);\n    }\n\n    int[] counts() {\n        return new int[0];\n    }\n\n    abstract void later();\n\n    private static class Inner {\n    }\n}\n\ninterface Named {\n    String name();\n}\n\nenum Mode { ASK }\nrecord Item(String name) {}\n@interface Marker {}\n";
        assert_eq!(
            of(Lang::Java, text),
            [
                "5: public final class Shop extends Base implements Named",
                "  9: public Shop(List<Item> items)",
                "  14: public double total(List<Item> items)",
                "  20: static <T> List<T> wrap(T one)",
                "  24: int[] counts()",
                "  30: private static class Inner",
                "34: interface Named",
                "38: enum Mode { ASK }",
                "39: record Item(String name) {}",
                "40: @interface Marker {}",
            ]
        );
    }

    #[test]
    fn c_and_cpp() {
        let text = "#include <stdio.h>\n#define LIMIT 4\n\ntypedef struct item {\n    int price;\n} item_t;\n\ntypedef unsigned long size_type;\nstruct point;\nenum class Mode { Ask };\n\nstatic int add(int a, int b)\n{\n    if (a > b) {\n    }\n    return a + b;\n}\n\nconst char *name_of(const item_t *item) {\n    return item->name;\n}\n\nint main(int argc, char **argv) {\n    printf(\"%d\", add(1, 2));\n    return 0;\n}\n\nnamespace shop {\nclass Store : public Base {\npublic:\n    double total() const;\n};\n}\n\ndouble Store::total() const {\n    return 0;\n}\n\ntemplate <typename T>\nstd::vector<T> wrap(T one) {\n}\nusing Done = void (*)();\nint declared(int a);\nx = compute(1);\n";
        assert_eq!(
            of(Lang::C, text),
            [
                "2: #define LIMIT 4",
                "4: typedef struct item",
                "8: typedef unsigned long size_type;",
                "10: enum class Mode { Ask };",
                "12: static int add(int a, int b)",
                "19: const char *name_of(const item_t *item)",
                "23: int main(int argc, char **argv)",
                "28: namespace shop",
                "29: class Store : public Base",
                "35: double Store::total() const",
                "40: std::vector<T> wrap(T one)",
                "42: using Done = void (*)();",
            ]
        );
    }

    #[test]
    fn svelte() {
        let text = "<script lang=\"ts\">\n\timport { onMount } from 'svelte';\n\n\tlet draft = $state('');\n\tlet { name, size = 16 }: Props = $props();\n\tconst working = $derived(isActive(task));\n\tlet plain = 3;\n\n\tfunction grow() {\n\t\tif (!input) return;\n\t}\n\n\tasync function send() {\n\t}\n\n\tconst short = (value: string, max = 64) => value;\n\texport const ideas = ['a'];\n</script>\n\n{#snippet diffView(diff: string)}\n\t<pre>{diff}</pre>\n{/snippet}\n\n<section class=\"screen\">\n\t{#snippet inner()}\n\t{/snippet}\n\tfunction notCode() {\n</section>\n\n<style>\n\t.code {\n\t}\n</style>\n";
        assert_eq!(
            of(Lang::Svelte, text),
            [
                "1: <script lang=\"ts\">",
                "4: let draft = $state('');",
                "5: let { name, size = 16 }: Props = $props();",
                "6: const working = $derived(isActive(task));",
                "9: function grow()",
                "13: async function send()",
                "16: const short = (value: string, max = 64) => value;",
                "17: export const ideas = ['a'];",
                "20: {#snippet diffView(diff: string)}",
                "25: {#snippet inner()}",
                "30: <style>",
            ]
        );
    }

    #[test]
    fn a_file_and_a_folder_are_outlined_and_other_files_are_named_as_such() {
        let base = files::scratch("outline");
        let root = base.join("project");
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn one() {}\n\nimpl Thing {\n    fn two(&self) {}\n}\n").unwrap();
        std::fs::write(root.join("src/deep/app.py"), "def main():\n    pass\n").unwrap();
        std::fs::write(root.join("target/built.rs"), "pub fn generated() {}\n").unwrap();
        std::fs::write(root.join("notes.txt"), "fn not_code() {}\n").unwrap();
        let many: String = (0..30).map(|i| format!("pub fn f{i}() {{}}\n")).collect();
        std::fs::write(root.join("src/many.rs"), many).unwrap();
        let f = Folder::open(root.to_str().unwrap()).unwrap();

        let one = outline(&f, "src/lib.rs").unwrap();
        assert_eq!(
            one,
            "src/lib.rs (5 lines)\n1: pub fn one() {}\n3: impl Thing\n  4: fn two(&self) {}\nRead a part with read_file and from_line."
        );
        assert!(outline(&f, "notes.txt").unwrap_err().contains("does not know this kind of file"));
        assert!(outline(&f, "../elsewhere.rs").is_err());

        let all = outline(&f, ".").unwrap();
        assert!(all.contains("src/lib.rs (5 lines)\n1: pub fn one() {}\n3: impl Thing"), "{all}");
        assert!(!all.contains("fn two"), "a folder shows the top level only: {all}");
        assert!(all.contains("src/deep/app.py (2 lines)\n1: def main():"), "{all}");
        assert!(all.contains("12: pub fn f11() {}\n[18 more: outline this file to see them]"), "{all}");
        assert!(!all.contains("generated") && !all.contains("not_code"), "{all}");
        assert!(outline(&f, "target").unwrap().contains("1: pub fn generated() {}"), "a folder named outright is looked into");
        assert!(outline(&f, "src/deep").unwrap().starts_with("src/deep/app.py (2 lines)"));
    }
}
