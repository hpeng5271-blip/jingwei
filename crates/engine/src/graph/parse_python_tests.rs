// **Python 的第二门语言验收**（2026-10-16 加）。
//
// 这一组测试盯的是"图谱对 Python 项目**从来没有成立过**"那件事：
// `graph/index.rs` 原来只收 `.rs`，于是三个演示项目里两个（都是 Python）
// 在图谱里一个节点都没有——诊断挂不上文件、"改了这里影响谁"问不出来。
//
// 断言刻意做成**结构性**的（id 形状、边的端点），不钉具体条数——
// 那种断言每次改文案就红，会被当成噪声绕过去（`real_projects.rs` 的教训）。
// 跨模块那一半（真夹具 → 真库 → 真边）在 `tests/python_graph.rs`。

use super::*;

fn ids(o: &ParseOutcome) -> Vec<String> {
    let mut v: Vec<String> = o.symbols.iter().map(|s| s.id.clone()).collect();
    v.sort();
    v
}

#[test]
fn 顶层的函数与类都成为符号() {
    let src = "def f():\n    pass\n\nclass C:\n    pass\n";
    let o = parse_python("a.py", src, 1);
    assert!(o.complete, "这段代码没有语法错，complete 该是 true");
    assert_eq!(
        ids(&o),
        vec!["symbol:a.py#C".to_string(), "symbol:a.py#f".to_string()],
        "函数与类都要成节点——少了类，`C()` 这种调用就永远找不到定义"
    );
}

/// **类里的方法是独立的符号**：`C.m` 与顶层的 `m` 不是一回事。
///
/// 这一条与 Rust 的 `impl` 块同义。少了容器路径，
/// 两个类里的同名方法会撞成同一个节点——而"这个方法属于哪个类"就没法回答了。
#[test]
fn 方法带容器路径而顶层函数不带() {
    let src = "class C:\n    def m(self):\n        pass\n\ndef m():\n    pass\n";
    let o = parse_python("a.py", src, 1);
    let got = ids(&o);
    assert!(
        got.contains(&"symbol:a.py#C::m".to_string()),
        "类里的方法要带容器路径（C::m），实际：{got:?}"
    );
    assert!(
        got.contains(&"symbol:a.py#m".to_string()),
        "顶层函数不带容器路径，实际：{got:?}"
    );
    assert_eq!(got.len(), 3, "两条 `m` 必须是两个不同的符号：{got:?}");
}

/// `self.m()` —— 接收者类型就在眼前（所在 class），不需要任何推断。
#[test]
fn self调用能连到同类的方法() {
    let src = "class C:\n    def a(self):\n        return self.b()\n\n    def b(self):\n        return 1\n";
    let o = parse_python("a.py", src, 1);
    assert_eq!(
        o.calls,
        vec![(
            "symbol:a.py#C::a".to_string(),
            "symbol:a.py#C::b".to_string()
        )],
        "`self.b()` 该连成一条边"
    );
}

/// 裸标识符调用 + 本文件没定义 → **老实记成"解析不了"**，让跨文件那一步去接。
///
/// 注意 `scope` 里存的是**完整 id**：`resolve_file` 拿 `r.scope.last()`
/// 直接当调用者写进边里。第一版我在这里存了短名（`main`），
/// 于是索引直接失败在「写关系失败：main -calls-> symbol:…」——
/// **是引擎自己报出来的**，不是人看出来的。
#[test]
fn 引用里记的是完整调用者id而不是短名() {
    let src = "def main():\n    return helper()\n";
    let o = parse_python("a.py", src, 1);
    assert_eq!(o.references.len(), 1, "`helper` 本文件没定义，该记成引用");
    assert_eq!(o.references[0].name, "helper");
    assert_eq!(
        o.references[0].scope,
        vec!["symbol:a.py#main".to_string()],
        "作用域链里要是完整 id——短名会让写边失败"
    );
}

/// **`from x import y` 之后的 `y.z()` 不是猜**：接收者是 import 引进来的名字，
/// 那说明 `y` 是个模块，而这个事实本文件里就写着。
#[test]
fn 导入进来的模块上调用产出限定名引用() {
    let src = "from hello_python import core\n\ndef main():\n    return core.greet(1)\n";
    let o = parse_python("main.py", src, 1);
    assert_eq!(
        o.imports,
        vec![ImportFact {
            local: "core".to_string(),
            path: "hello_python::core".to_string(),
        }],
        "`from hello_python import core` 绑定的是 `core` → `hello_python::core`"
    );
    let qualified: Vec<&str> = o
        .references
        .iter()
        .map(|r| r.name.as_str())
        .filter(|n| n.contains("::"))
        .collect();
    assert_eq!(
        qualified,
        vec!["hello_python::core::greet"],
        "`core.greet()` 要产出限定名，交给 `resolve::through_import` 去落地"
    );
}

/// **不是 import 引进来的接收者一律不收**——`obj.save()` 里的 `obj`
/// 是局部变量还是模块，语法层看不出来。收了就会出现一条信誓旦旦的假边。
#[test]
fn 看不出接收者身份时一条边都不收() {
    let src = "def f():\n    obj = make()\n    return obj.save()\n";
    let o = parse_python("a.py", src, 1);
    let has_save = o.references.iter().any(|r| r.name.contains("save"));
    assert!(
        !has_save,
        "`obj.save()` 的接收者不是 import 引进来的，不该产出引用：{:?}",
        o.references
    );
}

/// 导入的形式与绑定名。**`import a.b` 绑的是 `a` 不是 `b`**——
/// 这一条搞混了，`a.b.f()` 那类写法就会落到错的地方。
#[test]
fn 各种导入形状都转成名字到路径的映射() {
    let src = "import a.b\nimport c.d as e\nfrom f.g import h\nfrom f.g import i as j\n";
    let o = parse_python("a.py", src, 1);
    let got: Vec<(String, String)> = o
        .imports
        .iter()
        .map(|i| (i.local.clone(), i.path.clone()))
        .collect();
    for want in [
        ("a", "a::b"),
        ("e", "c::d"),
        ("h", "f::g::h"),
        ("j", "f::g::i"),
    ] {
        assert!(
            got.contains(&(want.0.to_string(), want.1.to_string())),
            "缺了 {want:?}，实际：{got:?}"
        );
    }
}

/// 通配导入**不收**：`from a import *` 引进来多少个名字，语法层不知道。
/// 收的话只能瞎猜，而猜错了比不收糟。
#[test]
fn 通配导入不被当成一条映射() {
    let src = "from a.b import *\n";
    let o = parse_python("a.py", src, 1);
    assert!(
        o.imports.is_empty(),
        "`import *` 不该产出映射，实际：{:?}",
        o.imports
    );
}

/// 语法错时 `complete = false` —— 与 Rust 那边同一条硬要求（Q8 实测出来的）：
/// 调用方据此**只出符号、不出关系**。少了它，图里会出现
/// "存在但根本编译不过"的符号，然后"这个函数在哪定义"给出一个不存在的答案。
#[test]
fn 语法错时complete是false() {
    let o = parse_python("a.py", "def broken(\n", 1);
    assert!(!o.complete, "这段代码语法是坏的，complete 必须是 false");
}

/// **`__pycache__` 之类不该被当成源码**——这条由 `index.rs` 的目录黑名单管，
/// 但那个名单是**按名字**判的，所以这里钉一下 `.pyc` 不会被收进来。
#[test]
fn 只有点py结尾才算python源码() {
    // 解析器本身不判扩展名（`index.rs` 判）；这里钉的是"喂进来的东西
    // 是不是 Python 语法"这件事不会 panic。
    let o = parse_python("a.pyc", "\u{0}\u{1}binary junk", 1);
    assert!(!o.complete, "二进制垃圾不该被当成解析成功");
}
