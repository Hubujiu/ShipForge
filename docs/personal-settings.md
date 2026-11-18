# Windows 个人偏好与快捷键

在「全部设置」的「你的设置」下打开「偏好设置」或「快捷键」。偏好归当前账号，在空间之间通用。本机工作区将设置写入 SQLite，团队模式使用令牌所属账号，演示模式只修改内存数据。

## 改变外观和语言

主题提供跟随系统、浅色、深色、魔法蓝和经典深色。选择「跟随系统」后，已打开的客户端会响应 Windows 的外观变化。「外边框颜色」提供无色和 12 个内置选项，影响标题栏及窗口边缘。常规设置可在简体中文与 English 之间切换。密度区保留参考界面的「即将推出」状态。

## 发送和应用快捷键

「Enter 发送」以 Shift+Enter 换行。「Shift+Enter 发送」以 Enter 换行。两种方式适用于对话、任务评论与运行回复。录制时按新组合键，按 Esc 取消。与其他应用命令冲突或与常用系统及编辑操作冲突时，保留原键位。每个命令都能独立恢复默认。

| 命令 | 默认 | 行为 |
| --- | --- | --- |
| 命令菜单 | Ctrl+K | 搜索工作区，或选择移动、当前视图查找。 |
| 将任务移动到空间 | Ctrl+Shift+M | 打开任务及目标空间选择器。任务详情中触发时会预选当前任务。 |
| 在当前视图查找 | Ctrl+F | 按任务标题或编号筛选当前列表。在其他页面触发时进入空间任务。清空文本可恢复原列表。 |

从任务详情执行「立即执行」会为处理该任务的 Agent 或小队排队一个运行。可以分别控制指派完成、执行快捷指令后是否打开该任务的会话。手动使用「任务会话」可回到已有讨论。

## CLI 协议

将下列 JSON 送入 `shipforge request`。读取偏好返回 `rev` 和 `preferences`。在更新时传回读到的 `expected_rev`，可避免另一客户端已保存的修改被覆盖。省略的字段保留原值。

```json
{"op":"preferences.get"}
```

```json
{"op":"preferences.update","expected_rev":1,"preferences":{"theme":"magic-blue","frame_color":"color-6","language":"en","send_with":"shift-enter","open_after_assignment":true,"open_after_command":false,"shortcuts":{"command_menu":"Alt+Shift+J"}}}
```

| 字段 | 值 |
| --- | --- |
| `theme` | `system`、`light`、`dark`、`magic-blue`、`classic-dark` |
| `frame_color` | `none` 或 `color-1` 至 `color-12` |
| `language` | `zh-Hans` 或 `en` |
| `send_with` | `enter` 或 `shift-enter` |
| `open_after_assignment`、`open_after_command` | 布尔值 |
| `shortcuts` | `command_menu`、`move_task`、`find_in_view`。单项传 `null` 恢复默认。 |

任务操作示例使用当前空间的真实任务 ID。移动保留任务 ID、描述、评论和附件，目的地分配自己的任务编号。操作会校验来源与目标权限、目标空间必填字段和版本，遇到仍绑定空间的执行历史、外部来源或任务关系时返回冲突。任务会话按成员、任务和执行者复用。快捷指令的响应包含 `chat` 和 `run`，相同 `request_id` 的重试返回同一运行。

```json
{"op":"task.move","space":"default","id":"task-id","target":"delivery","expected_rev":2}
```

```json
{"op":"task.conversation","space":"delivery","id":"task-id"}
```

```json
{"op":"task.command","space":"delivery","id":"task-id","instruction":"汇总待处理事项","request_id":"summary-001"}
```
