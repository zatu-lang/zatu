# zatu

**雑に書けば、動く。**

7歳でも、難しい概念を覚えずに、思いついたプログラムを書いて動かせることを目指す実験的なプログラミング言語です。

設計の考え方は [docs/言語設計.md](docs/言語設計.md) にあります。いまは v0（とりあえず動く版）で、文法はすべて仮決定です。

```zatu
type User {
    name string
    age int
}

users = [User("ようかん", 7), User("たろう", 20)]

for user at i in users {
    if user.age >= 18 {
        print("{i}: {user.name} はおとな")
    }
}
```

## 動かし方

[mise](https://mise.jdx.dev/) で Rust を入れます。

```sh
mise install
mise exec -- cargo run -- examples/hello.zt
mise exec -- cargo test
```

## エラーの例

間違いがあると、実行する前に止めて教えます。

```text
エラー: typo.zt:3:5
 3 |     coutn = count + n
   |     ^^^^^
coutn に値を入れていますが、一度も使っていません
ヒント: count のまちがいではありませんか？

直すところが 1 個あるので、実行しませんでした。
```
