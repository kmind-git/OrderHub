# 编译缓存清理

执行日期：2026-10-11。用户要求处理编译产物造成的磁盘不足。

- `target/debug` 不存在；主要占用来自 nextest 的增量编译缓存。
- 删除前扫描 `target/nextest/incremental` 为 35,580,764,812 字节。
- 清理前核对绝对路径位于本仓库，拒绝重解析点，停止本次验收脚本及其 Cargo 子进程。
- release 矩阵在独立目录运行，未停止。源码、历史验收记录及 release 产物保留。
- 清理后增量缓存目录消失；D 盘可用空间由约 12 GiB 增至约 45 GiB，净释放约 33 GiB。
  净值包含清理期间 release 构建的并发写入，精确数值见 [清理结果](incremental-cleanup.json)。
- nextest profile 及本次验收脚本禁用增量缓存；开发说明和 CI 使用 `--profile nextest`，
  Makefile 的构建、检查、工具及文档入口使用默认 `PROFILE=nextest`。
  需要调试符号时可显式传入 `PROFILE=dev`。
- 清理后的 Cargo metadata 与格式检查返回 0；P8 测试从中断的 P5 API 门禁恢复。

实际清理脚本见 [cleanup-incremental.ps1](cleanup-incremental.ps1)，没有清理交易数据或源码。

## 验收后清理

- P8 全部必需门禁完成，源码/文档摘要及 37 个归档产物完整性复核通过后，
  删除本轮专用目录 `audit/orderhub-simplification-2026-10-10/run-18/build-target`。
- 删除前核对该绝对路径在本仓库内、没有重解析点，且没有进程命令行使用该目录。
- 本次净释放 12,772,995,072 字节（约 11.90 GiB），D 盘剩余 53,540,274,176 字节
  （约 49.86 GiB）。两次清理合计净释放 48,209,399,808 字节（约 44.90 GiB）。
  这不是全盘空间变化统计；其间的验证构建也会写入磁盘。
- 源码、交易/测试数据、全部日志、生成协议归档及 37 个真实产物保留；
  根目录 `target/nextest` 的非增量缓存保留，后续开发可复用。
- 文档检查使用归档的 `orderhub-check.exe`，清理后仍可复核验收证据。

脚本见 [cleanup-matrix.ps1](cleanup-matrix.ps1)，实际结果见
[matrix-cleanup.json](matrix-cleanup.json)。清理后的摘要和本地链接复核见
[final-state-validation.json](final-state-validation.json)。
