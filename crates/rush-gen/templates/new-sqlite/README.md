# @@NAME@@

RushWind 服务脚手架（`rush new --storage sqlite` 生成）：**内嵌 SQLite**
存储——单连接内存库，零安装零 DSN，开箱即跑；表在启动时 `migrate_create`
自动建立。

## 运行

```shell
cargo run
# [@@NAME@@] serving on 127.0.0.1:<port>
curl http://127.0.0.1:<port>/health   # ok
curl http://127.0.0.1:<port>/wired    # repo=true（存储已接入）
curl -X POST http://127.0.0.1:<port>/items -H 'Content-Type: application/json' \
  -d '{"name":"hello"}'
curl http://127.0.0.1:<port>/items
```

## 换持久化

`sqlite_memory` 是单连接内存库（进程退出即失）。要落盘，把 `src/main.rs`
工厂里的连接换成：

```rust
let repo = SeaRepo::connect("sqlite://data.db?mode=rwc", schema()).await?;
```

（SeaORM 支持的任意后端同理：`postgres://…`、`mysql://…`。）
