# P8 原生实现来源审查

审查平台：本机 Windows x64 / stable Rust 1.98.1 / MSVC。
依赖、构建脚本、原生候选文件及选中 feature 的逐文件摘要见本目录 JSON 清单；
最终构建、链接及运行结果另见 P8 验收记录。未选中的包不计为已构建通过。

## 实际依赖图中的候选来源

| 来源                        | 判定依据                                                                                                                     |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| BLAKE3 的 C/汇编文件        | 本地补丁的 pure 分支在 C 编译器探测前返回；最终 feature 图选中 pure，保留 Rust SIMD 实现。未选中原 C/外部汇编构建分支。      |
| cc 的 detect_compiler.c     | cc 本身为 Rust 构建库；BLAKE3 pure 路径不调用其探测/编译分支。实际调用哨兵及链接输入另外核对，不能仅凭 cc 包名判定。         |
| implied-vol 的 C++ 参考源码 | cxx_bench 是依赖自身的可选 feature，不由 workspace --all-features 自动开启；实际图未选中，build.rs 的非 cxx_bench 分支为空。 |
| sysinfo 的 C 示例及头文件   | 不是本项目 Cargo 目标，也不是 build.rs 输入；运行路径使用 Rust 实现及系统 API。                                              |
| ustr 头文件                 | 接口声明文件，不包含实际选中的 C 实现；Cargo 构建及链接输入须保持无第三方原生库。                                            |
| walkdir 的 C 比较程序       | 上游比较示例，不是本项目支持的构建或 benchmark 入口。                                                                        |
| Graviola                    | Rust 源文件内的 inline assembly 独立登记；没有外部 C/C++ 编译器或汇编器构建步骤。真实 CPU 门禁及 TLS 用例验证本机选中路径。  |

## 生成、工具与平台边界

- orderhub-proto 使用 Rust protox、prost/tonic 生成器，不执行 C++ protoc。
- core/common 等构建脚本的 cfg、Rust 代码、版本探测及 Git 来源读取属于构建工具链。
  Git、PowerShell/Bash、MSVC 工具和 Windows SDK 是本地开发工具，不作为 OMS 第三方实现随程序装载。
- winapi/ntapi/windows-*、Rust std 的 extern ABI 和平台导入库调用 Windows API；
  Rust 源码导出 C ABI 不等于嵌入 C 实现。选中的系统库逐项登记在链接证据中。
  CredUI 的 SDK 导入库及系统 DLL 路径/摘要另见
  [平台文件证据](credential-ui-platform-files.json)。链接参数 `/IMPLIB` 为生成输出，
  不作为第三方输入库；首次分析结果仍保留，最终分类见 [native-review.json](native-review.json)。
- OrderHub 生产代码没有自定义 LoadLibrary/dlopen/libloading 插件入口；P7 已退役动态插件框架。
  实际 B/G/U release 节点的模块观测用于核对可触达的运行加载。
- 数据库生产驱动为 Rust SQLx/redis-rs；其 TLS 使用共同 provider。
  它们不加载 PostgreSQL/Redis 服务实现到本机进程，真实服务是另行验收的外部程序。
- 旧 Iai/Valgrind 和 CodSpeed 编译入口已退役；五个原 benchmark 文件的原计算和断言迁到原生 Criterion。
  Criterion 0.7.0 的最小补丁禁止探测/启动外部 gnuplot；保留文本和 JSON 结果。
  补丁许可证、原 manifest 及源差异见本目录 baseline、criterion-source.patch 和补丁说明。
- Parquet 的历史读取使用保留的 ruzstd 补丁；不启用上游 Zstandard writer，Arrow/Parquet 不进入默认 node。
  没有另引入分配器或 C 压缩实现；历史兼容验证与 release 资源观测单独记录。

## 证据边界

编译器哨兵不能发现所有预编译库或动态加载，因此结合实际 -vv 日志、build.rs、
静态/动态链接输入、产物导入表及运行模块观测判定；未分类项不能记为通过。
存在于锁文件/解析 metadata 但没有被当前 Windows 图选中的包单独列为 unselected。
本期结论只适用于已选平台和配置，不把 Windows 验收当作 Linux/macOS 或部署安全认证。
Graviola 清单登记的 139 个含 asm 的 Rust 源文件包含未选中的其他架构；
清单完整性不等于所有架构的编译或运行验证。
