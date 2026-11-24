// 言語の動作テスト。実際の zatu コードを動かして、出力やエラーを確かめる。

use zatu::Failure;

fn run_with_input(src: &str, input: &str) -> Result<String, String> {
    let mut out = Vec::new();
    let mut inp = input.as_bytes();
    match zatu::run(src, &mut out, &mut inp) {
        Ok(()) => Ok(String::from_utf8(out).unwrap()),
        Err(Failure::NotRun(errs)) => Err(errs.iter().map(|e| e.msg.clone()).collect::<Vec<_>>().join("\n")),
        Err(Failure::Runtime(e)) => Err(e.msg),
    }
}

fn ok(src: &str) -> String {
    run_with_input(src, "").unwrap_or_else(|e| panic!("エラーになった: {e}"))
}

fn fails(src: &str) -> String {
    match run_with_input(src, "") {
        Ok(out) => panic!("エラーになるはずが成功した: {out}"),
        Err(e) => e,
    }
}

#[test]
fn examples() {
    assert_eq!(
        ok(include_str!("../examples/hello.zt")),
        "こんにちは、ようかん！\nようかんは7歳です\n「ようかん」は 4 文字\n"
    );
    assert_eq!(ok(include_str!("../examples/adults.zt")), "1: たろう\n2: はなこ\n");
    assert_eq!(ok(include_str!("../examples/kuku.zt")).lines().nth(8), Some("9 18 27 36 45 54 63 72 81 "));
    assert_eq!(ok(include_str!("../examples/fizzbuzz.zt")).lines().last(), Some("FizzBuzz"));
    assert_eq!(
        ok(include_str!("../examples/birthday.zt")),
        "7 8\nやあ、ようかん（8歳）\nやあ、ようかん（8歳）\n"
    );
}

#[test]
fn kazuate_with_input() {
    // random は毎回変わるので、1〜100 を順に入れれば必ず当たる
    let input: String = (1..=100).map(|n| format!("{n}\n")).collect();
    let out = run_with_input(include_str!("../examples/kazuate.zt"), &input).unwrap();
    assert!(out.contains("あたり！"), "{out}");
}

#[test]
fn value_independence() {
    let src = r#"
type User {
    name string
    age int
}
a = User("ようかん", 7)
b = a
b.age = 8
print(a.age, b.age)

xs = [[1, 2], [3, 4]]
ys = xs
ys[1][1] = 0
print(xs, ys)
"#;
    assert_eq!(ok(src), "7 8\n[[1, 2], [3, 4]] [[0, 2], [3, 4]]\n");
}

#[test]
fn named_fields_use_the_assignment_type() {
    let src = r#"
type 人 {
    名前 string
    年齢 int
}
type 組 { みんな []人 }
func 作る() 組 {
    一人: 人 = {
        年齢: 7,
        名前: "ようかん",
    }
    結果: 組 = { みんな: [一人] }
    結果.みんな += { 名前: "たろう", 年齢: 20 }
    結果.みんな[1] = { 名前: 一人.名前, 年齢: 8 }
    return 結果
}
組のみんな = 作る().みんな
print(組のみんな[1].名前, 組のみんな[1].年齢, 組のみんな[2].名前)
"#;
    assert_eq!(ok(src), "ようかん 8 たろう\n");
}

#[test]
fn array_annotations_survive_empty_assignments_and_nested_updates() {
    assert_eq!(
        ok("type User { age int }\nusers: []User = []\nusers += { age: 7 }\nprint(users[1].age)"),
        "7\n"
    );
    for update in ["users += 7", "users = [7]", "users = 1..2"] {
        let src = format!("type User {{ age int }}\nusers: []User = [User(1)]\nusers = []\n{update}\nprint(users)");
        assert!(fails(&src).contains("User"), "{update}");
    }
    assert!(fails("rows: [][]int = [[]]\nrows[1] += \"a\"\nprint(rows)").contains("ここには int が入ります"));
    assert!(fails("type User { age int }\nu: User = { age: \"7\" }\nprint(u)").contains("ここには int が入ります"));
}

#[test]
fn named_field_typos_are_reported_before_running() {
    let src = "type 人 { 年齢 int }\nprint(1)\n一人: 人 = { 年令: 7 }\nprint(一人)";
    let mut out = Vec::new();
    let Err(Failure::NotRun(errs)) = zatu::run(src, &mut out, &mut "".as_bytes()) else {
        panic!("実行前にエラーになるはず");
    };
    assert!(out.is_empty());
    assert_eq!(errs[0].span, zatu::ast::Span::new(3, 11, 2));
    assert!(errs[0].msg.contains("年令 という項目はありません"));
    assert!(errs[0].hint.as_deref().unwrap().contains("年齢 のまちがい"));
}

#[test]
fn named_fields_require_a_known_type_and_each_field_once() {
    let cases = [
        ("user = { age: 7 }", "どの型の値を作るか分かりません"),
        ("user: User = {}", "age の値が必要です"),
        ("user: User = { age: 7, age: 8 }", "age が2回書かれています"),
        ("user: Usre = User(7)", "Usre という型はありません"),
        ("user: User = User(7)\nuser: int = 7", "user の型は User と書かれています"),
    ];
    for (assignment, message) in cases {
        let error = fails(&format!("type User {{ age int }}\n{assignment}\nprint(user)"));
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn strings_count_characters() {
    assert_eq!(ok(r#"s = "ようかん"
print(len(s), s[1], s[4])"#), "4 よ ん\n");
    assert_eq!(ok(r#"print(len("🇯🇵"))"#), "1\n");
    assert_eq!(ok(r#"for c in "あい" {
    print(c)
}"#), "あ\nい\n");
}

#[test]
fn indexes_start_at_one() {
    assert_eq!(ok("xs = [10, 20, 30]\nfor x at i in xs {\n    print(i, x)\n}"), "1 10\n2 20\n3 30\n");
    assert!(fails("xs = [1]\nprint(xs[0])").contains("1 から"));
    assert!(fails("xs = [1]\nprint(xs[2])").contains("1 個しかない"));
}

#[test]
fn ufcs() {
    let src = "func double(n) {\n    return n * 2\n}\nprint(3.double(), double(3), [1, 2].len())";
    assert_eq!(ok(src), "6 6 2\n");
}

#[test]
fn loops() {
    let src = r#"
n = 0
for n < 3 {
    n += 1
}
for {
    n += 10
    if n > 30 {
        break
    }
}
print(n)
for i in 1..5 {
    if i % 2 == 0 {
        continue
    }
    print(i)
}
"#;
    assert_eq!(ok(src), "33\n1\n3\n5\n");
}

#[test]
fn modifying_array_while_looping_does_not_change_iterations() {
    let src = "xs = [1, 2]\nfor x in xs {\n    xs += x\n}\nprint(xs)";
    assert_eq!(ok(src), "[1, 2, 1, 2]\n");
}

#[test]
fn else_on_next_line() {
    assert_eq!(ok("if false {\n    print(1)\n}\nelse {\n    print(2)\n}"), "2\n");
}

#[test]
fn recursion() {
    let src = "func fib(n) {\n    if n <= 2 {\n        return 1\n    }\n    return fib(n - 1) + fib(n - 2)\n}\nprint(fib(20))";
    assert_eq!(ok(src), "6765\n");
}

// ---- 実行前に止まるエラー ----

#[test]
fn typo_on_write_is_caught() {
    let e = fails("count = 0\nfor n in 1..3 {\n    coutn = count + n\n}\nprint(count)");
    assert!(e.contains("coutn に値を入れていますが、一度も使っていません"), "{e}");
}

#[test]
fn typo_on_read_is_caught() {
    assert!(fails("count = 1\nprint(cuont)").contains("cuont という変数はまだ作られていません"));
}

#[test]
fn errors_stop_before_running() {
    // 1行目の print も実行されないこと
    let mut out = Vec::new();
    let r = zatu::run("print(1)\nprint(nai)", &mut out, &mut "".as_bytes());
    assert!(matches!(r, Err(Failure::NotRun(_))));
    assert!(out.is_empty());
}

#[test]
fn loop_variable_cannot_be_changed() {
    let e = fails("type U { age int }\nus = [U(1)]\nfor u in us {\n    u.age += 1\n}\nprint(us)");
    assert!(e.contains("ループの変数なので変更できません"), "{e}");
}

#[test]
fn discarded_result_is_caught() {
    let e = fails("func inc(n) {\n    return n + 1\n}\na = 1\ninc(a)\nprint(a)");
    assert!(e.contains("結果を使っていません"), "{e}");
}

#[test]
fn using_result_of_print_is_caught() {
    assert!(fails("x = print(1)\nprint(x)").contains("値を返さない"));
}

#[test]
fn arity_is_checked() {
    assert!(fails("type U { a int, b int }\nu = U(1)\nprint(u)").contains("2 個の値が必要"));
}

#[test]
fn functions_cannot_see_outer_variables() {
    let e = fails("total = 1\nfunc f() {\n    print(total)\n}\nf()\nprint(total)");
    assert!(e.contains("total という変数はまだ作られていません"), "{e}");
}

#[test]
fn colon_equals_is_explained() {
    assert!(fails("x := 1").contains(":= は使いません"));
}

#[test]
fn assignment_in_condition_is_explained() {
    assert!(fails("x = 1\nif x = 1 {\n    print(x)\n}").contains("条件の中で = は使えません"));
}

#[test]
fn fullwidth_symbols_are_explained() {
    assert!(fails("print（1）").contains("全角"));
}

// ---- 実行中のエラー ----

#[test]
fn inexact_division_is_an_error() {
    assert!(fails("print(7 / 2)").contains("割り切れません"));
    assert_eq!(ok("print(6 / 2, 7 % 2)"), "3 1\n");
}

#[test]
fn changing_kind_is_an_error() {
    assert!(fails("x = 1\nx = \"a\"\nprint(x)").contains("x には数が入っています"));
}

#[test]
fn arrays_hold_one_kind() {
    assert!(fails("xs = [1]\nxs += [2, 3]\nprint(xs)").contains("この配列には数が入っています"));
    assert!(fails("xs = [1, \"a\"]\nprint(xs)").contains("この配列には数が入っています"));
}

#[test]
fn string_plus_number_suggests_interpolation() {
    assert!(fails("age = 7\nprint(\"年齢\" + age)").contains("+ でつなげられません"));
}

#[test]
fn overflow_is_an_error() {
    assert!(fails("print(9223372036854775807 + 1)").contains("大きすぎ"));
}

#[test]
fn infinite_recursion_is_explained() {
    assert!(fails("func f(n) {\n    return f(n + 1)\n}\nprint(f(1))").contains("深くなりすぎ"));
}
