# 并发控制设计：路径锁、条件写入（ETag/If-Match）、原子替换

## 概述

当前实现为 AxoDrive 提供三层并发控制手段：

- 按路径加锁，序列化冲突写操作。
- 基于 ETag 的条件写入（If-Match / If-None-Match）。
- 原子替换（临时文件 + rename）。

目标是避免多用户同时操作同一路径导致的写入交错或覆盖。

## 目标

- 防止同一路径的并发写入交错。
- 让覆盖行为变成“显式且可检测”的条件更新。
- 确保上传完成后的替换是原子的。
- 尽量少改动现有 API 行为与前端流程。

## 非目标

- 多实例部署下的分布式锁。
- 完整的版本历史与冲突合并。
- 以内容哈希为基础的强 ETag。

## 术语

- 路径锁：按存储相对路径建立的进程内异步互斥锁。
- ETag：由文件元数据生成的验证字符串。
- 条件写入：只有满足预条件才允许写入。
- 原子替换：写入临时文件再 rename 覆盖目标。

## 按路径加锁（已实现）

### 范围

对会修改目标路径的操作加锁，例如：写入、删除、完成上传、创建目录。

### 键规则

- 使用存储相对路径，统一为 `/` 分隔。
- 目录操作使用目录路径作为锁键。
- 文件写/删使用文件路径作为锁键。

### 存储方式

- 进程内 `HashMap<String, Arc<tokio::sync::Mutex<()>>>`。
- 锁仅存在内存中，未做 TTL 清理。

### 用法

- 提供 `lock_path_with_timeout(path, timeout)`，超时返回 `409 Conflict`。
- 锁持有时间尽量短，仅覆盖关键区间。

### 安全性

- 一次只锁单一路径，避免死锁。
- 目前未覆盖多路径加锁（如 MOVE/rename）的排序策略。

## 条件写入（ETag / If-Match / If-None-Match，已实现）

### ETag 生成

弱 ETag，基于元数据避免全量哈希：

- `W/"<size>-<mtime-secs>-<mtime-nanos>"`
- 如果缺失 mtime，则回退到 size。

### 读接口

- `GET /api/files/download` 返回 `ETag` 与 `Last-Modified`。
- `GET /api/files/list` 已返回 `etag` 字段（目录为 `null`）。

### 写接口

- `PUT /api/files/write` 支持：
  - `If-Match`：仅当 ETag 匹配才允许覆盖。
  - `If-None-Match: *`：仅当文件不存在才允许创建。
- `POST /api/upload/complete` 同样支持上述条件头。
- `PUT /api/files/write` 与 `POST /api/upload/complete` 会返回 `ETag/Last-Modified`。

### 返回码约定

- `412 Precondition Failed`：条件不满足（ETag 不匹配/文件已存在）。
- `409 Conflict`：路径锁在超时内未获取到。

## 原子替换（已实现）

### 写入流程

1. 获取路径锁。
2. 解析目标路径并确保父目录存在。
3. 写入 `.<name>.tmp.<uuid>` 临时文件（同目录）。
4. 视平台支持情况 `fsync` 临时文件与父目录。
5. rename 原子替换目标文件。

### 上传完成流程

1. 获取路径锁。
2. 校验分片与总大小。
3. 合并写入目标目录的临时文件。
4. `fsync` 并 rename 替换。
5. 清理上传临时目录。

### 平台说明

- Windows 需使用支持覆盖的 rename/replace 语义。
- 临时文件必须与目标在同一文件系统，保证原子性。

## WebDAV 锁（已实现）

- 替换 `FakeLs` 为自实现内存锁系统（`WebDavLockSystem`）。
- 支持锁超时与续租，超时锁在每次访问时清理。
- WebDAV `LOCK/UNLOCK` 基于 WebDAV 路径锁定，与 API 路径锁是独立体系。

## 前端冲突展示（已实现）

### 触发场景

- `412 Precondition Failed`：ETag 不匹配或文件已存在。
- `409 Conflict`：路径锁超时。

### 展示策略

- 上传完成失败弹出冲突提示，显示“你的版本已过期/文件已被更新”。
- 提供当前文件元信息（大小、修改时间、ETag）供用户确认。
- 给出明确动作选项：
  - 重新加载并重试（获取新 ETag）。
  - 强制覆盖（去掉 If-Match）。
  - 另存为新文件（自动改名）。

### 文案建议

- 标题：`文件冲突`。
- 正文：`该文件已在你上传期间被其他人修改。请选择处理方式。`

## 代码改动清单（已完成）

- 新增 `LockManager`（`src/locking.rs`）。
- 新增原子写入与 ETag 模块：
  - `AtomicFile`
  - `etag_from_metadata`
  - `check_preconditions`
- 新增 WebDAV 锁系统：`WebDavLockSystem`

## 清理与 TTL

- 路径锁表未设置 TTL，可能持续增长（需要后续补充清理策略）。
- 上传临时目录清理保持不变，且不应在路径锁内执行。

## 测试计划

- 并发写入不应交错。
- If-Match 旧 ETag 必须被拒。
- If-None-Match: * 在文件已存在时拒绝。
- 上传完成使用原子替换，崩溃时目标不应变更。
- WebDAV 锁冲突能够阻止写入。

## 已知不足与后续建议

- 路径锁仅支持单一路径，未实现多路径操作的排序与合并锁。
- 路径锁无 TTL 清理，长时间运行可能积累空锁。
- WebDAV 锁与 API 路径锁互不感知，跨协议并发仍可能存在空窗。
- 前端目前仅对上传完成做冲突提示，若新增直接写入接口需补齐条件头与 412 提示。
