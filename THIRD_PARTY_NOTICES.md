# 第三方组件

本应用使用 Rust 开源组件，包括 anyhow、axum、tokio、reqwest、serde、serde_json、uuid、chrono 及其传递依赖，版本以 Cargo.lock 为准。许可由各上游分别提供。

Hub 接入适配代码源自 MIT 许可的 ZECTRIX Vocabulary；像素宠物几何沿用此前 ZECTRIX AI 工作搭子原型。界面和运行逻辑使用本项目 MIT 许可。

发行包不包含 Hub、设备 SDK、字体文件、模型或固件。设备画面渲染及其字体由用户已有的 Hub 提供，分别遵循 Hub 中的许可说明。
