# 离线工具验收脚本首次处置

2026-10-11 将策略/schema 与尚在构建的历史 release 转换分开核验。
PowerShell 的 if 表达式返回空数组时发生展开，results 被赋为 null；第一个结果随后变为单对象，
第二次追加时报 op_Addition 错误。应用没有在该错误中执行数据库连接。

修正为先显式初始化数组，再按 legacy 模式读取已有数组；仅重跑离线工具门禁。
最终状态以 offline-cli-validation.json 为准，不把脚本错误计为应用行为失败或成功。
