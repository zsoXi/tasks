# tasks

tasks keeps a small task list in a JSON file. Tasks can have subtasks, and each task belongs to one of six groups, always shown in this order: `now`, `later`, `ongoing`, `ideas`, `done`, `rejected`. Move a task to `done` to finish it. Only subtasks have a completion checkbox.

The workspace contains `tasks-core`, an in-memory library with no file, clock or network access, and `tasks`, a file-backed library with a thin CLI. A backend can link the core library directly, so editing a list does not need a subprocess. Both crates use the `MIT OR Apache-2.0` licence.

## CLI

Build with `cargo build --release`. Run requests from the folder that holds your lists:

```sh
tasks '{"op":"create","output":"launch.json","title":"Launch"}'
tasks '{"op":"add","input":"launch.json","expected_revision":0,"group":"now","title":"Send invoice"}'
printf '%s' '{"op":"read","input":"launch.json"}' | tasks
tasks --schema
tasks --messages
```

Pass one JSON argument, or send the same request on standard input. The CLI writes JSON to standard output and exits with 0 on success or 1 on error. File names must be relative to the working folder. Absolute paths, parent traversal and symbolic links are rejected, including dangling links and links in parent folders. Reads accept only regular files. The default input limit is 1 MiB, and a write that would make a list unreadable under that limit is rejected.

File access uses Unix directory descriptors and requires a Unix host. Reads check file type and size before opening and on the opened descriptor, use nonblocking opens, and cap the bytes read. Writes use a new temporary file in the destination directory, sync it, then rename it relative to the same directory descriptor. Cooperating writers lock the directory through the full read, revision check and replacement. The lock survives file replacement and works across processes. It does not coordinate with programs that ignore the lock. A reader sees one complete version. A failed write leaves the previous file intact.

## Responses and the read shape

A successful response has `ok`, `summary`, `summary_code`, `summary_params`, `warnings` and `data`. An error has the same status and message fields, without `data`. `warnings` is always an array, even when empty. Messages come from one English catalogue, with English as the fallback for an unsupported locale.

Create, read and edit requests return the full list in this shape:

```json
{
  "list": {"id": "l1", "title": "Launch", "revision": 2},
  "groups": [
    {"key": "now", "tasks": [
      {"id": "t1", "title": "Send invoice", "subtasks": [
        {"id": "t2", "title": "Check VAT", "done": false}
      ], "progress": {"done": 0, "total": 1}}
    ]},
    {"key": "later", "tasks": []},
    {"key": "ongoing", "tasks": []},
    {"key": "ideas", "tasks": []},
    {"key": "done", "tasks": []},
    {"key": "rejected", "tasks": []}
  ]
}
```

A note is included only when present. A task with no subtasks has no `progress` member. The saved `tasks/1` document also includes creation and update times and the next identifier counter. Deleted identifiers are never reused. The tool starts lists with the identifier `l1`, while a backend supplies its own list identifier through `new_list`. People, ownership and visibility belong in the backend, not the document.

## Operations

Every edit needs `input` and `expected_revision`. A successful edit increments the revision by exactly one, including an idempotent checkbox retry with the current revision. Retrying with an old revision returns `tasks.conflict` with `expected` and `actual`. It does not change the file.

| Operation | Other fields | Result |
| --- | --- | --- |
| `create` | `output`, `title` | Start a list at revision zero. Replaces an existing regular file atomically. |
| `read` | `input` | Read the list. No revision needed. |
| `add` | `title`, `group`, optional `note`, `index`, `parent` | Add a task, or a subtask when a parent task is supplied. The group is ignored for subtasks. Default position is last. |
| `edit` | `id`, optional `title`, `note` | Change a task or subtask. An omitted note stays unchanged. Use `null` to clear it. Subtasks cannot have notes. |
| `check` | `id`, `done` | Set a subtask's checkbox. |
| `move` | `id`, `group`, `index` | Move a task or reorder it. Positions start at zero, measured after removal. |
| `indent` | `id` | Make a task the last subtask of the task above it. |
| `outdent` | `id` | Promote a subtask directly after its parent. A completed subtask loses its checkbox and emits `tasks.done_flag_dropped`. |
| `delete` | `id` | Delete a task and its subtasks, or one subtask. |
| `rename` | `title` | Rename the list. |
| `snapshot` | `input`, `output`, optional `groups`, `show_notes`, `show_subtasks` | Save an HTML preview. No revision needed. |

## Limits and errors

Titles are trimmed and contain 1 to 200 Unicode scalar values, with no control characters. Notes contain up to 2,000 scalar values and may contain line breaks and tabs. Each task has at most 50 subtasks. A list has at most 500 tasks across all groups, without counting subtasks toward that total. The same rules apply when parsing a saved document.

| Code | Meaning |
| --- | --- |
| `tasks.title_empty`, `tasks.title_too_long` | A title is empty or exceeds 200 characters. |
| `tasks.note_too_long` | A note exceeds 2,000 characters. |
| `tasks.too_many_subtasks`, `tasks.too_many_tasks` | The edit exceeds 50 subtasks or 500 tasks. |
| `tasks.control_character` | A title or note contains a forbidden control character. |
| `tasks.conflict` | The saved revision differs from the expected revision. |
| `tasks.not_found` | An identifier was not found. See `id` for the missing identifier. |
| `tasks.subtask_note`, `tasks.not_subtask` | The operation does not fit the selected task or subtask. |
| `tasks.index_out_of_range` | A position is beyond the end. Positions are never clamped. |
| `tasks.no_task_above`, `tasks.has_subtasks`, `tasks.has_note` | Indentation would have no parent or would lose information. Clear even an empty note explicitly before indenting. |
| `tasks.validation`, `tasks.enum` | A field is missing or invalid. `field` is a JSON Pointer. Enum errors also include an `allowed` array. |
| `tasks.format`, `tasks.invalid_ids`, `tasks.groups` | The saved format, identifiers or fixed groups are invalid. |
| `tasks.counter_exhausted` | A revision or identifier cannot be incremented. |
| `tasks.invalid_path`, `tasks.symlink`, `tasks.not_regular_file` | The file path or file type is not allowed. |
| `tasks.input_too_large` | The configured byte limit was exceeded. |
| `tasks.invalid_json`, `tasks.invalid_utf8`, `tasks.arguments` | The request is not usable text or has extra arguments. |
| `tasks.io`, `tasks.clock` | A file operation or clock read failed. The library preserves the underlying error. |

Limit errors include `params.limit` in the core library and `summary_params.limit` in tool responses. `--messages` lists every error, success and warning code with its template and required parameters. `--schema` describes the request forms, including file pickers. Unknown fields are rejected.

## Read-only snapshots

```sh
tasks '{"op":"snapshot","input":"launch.json","output":"preview.html","groups":["now","done"],"show_notes":false,"show_subtasks":true}'
```

Snapshots include all groups by default. Notes are hidden by default, and subtasks are shown. The HTML contains escaped text, one inline stylesheet, task counts and progress bars. It contains no scripts, event attributes, external resources, document identifiers or revision. The renderer cannot access backend ownership or visibility data.

The response includes `data.style_sha256`, the base64 SHA-256 hash of the exact stylesheet bytes. A server can serve the page with:

```text
Content-Security-Policy: default-src 'none'; style-src 'sha256-HASH_FROM_RESPONSE'
```

Progress widths use stylesheet classes, not inline style attributes, so they work under this policy. A snapshot is a copy, not a live view. The server still controls who can open the preview link.

## Use tasks-core in a backend

```rust
use tasks_core::{Edit, GroupKey, Timestamp, apply, new_list, parse, to_json, view};

fn example() -> Result<(), Box<dyn std::error::Error>> {
    let now = Timestamp::new("2026-10-15T18:00:00Z")?;
    let list = new_list("launch", "Launch", now.clone())?;
    let next = apply(&list, &Edit::Add {
        expected_revision: 0,
        group: Some(GroupKey::Now),
        title: "Send invoice".into(),
        note: None,
        index: None,
        parent: None,
    }, now)?;
    let saved = to_json(&next);
    let restored = parse(&saved)?;
    let response = view(&restored);
    assert_eq!(response["list"]["revision"], 1);
    Ok(())
}
```

`Document` has private state and validates on deserialization. `parse` validates the shape, fixed groups, identifier counter and limits. `apply` returns a new document, leaving its input untouched. `to_json` is deterministic. The caller supplies time through `Timestamp`, which accepts UTC timestamps with whole seconds and a literal `Z`. `apply_with_warnings` also returns the outdent warning. `snapshot_html`, `SnapshotOptions`, `schema`, `messages::catalogue()` and the four `MAX_*` limit constants are available from the same crate.

For file-backed use, call `tasks::run(&Input, &Options)`. `Options` sets the working folder, maximum bytes, locale and optional timestamp. Without a supplied timestamp, this outer library reads the system clock. A backend using its own storage must protect the read, revision check and commit in one transaction.

## Checks

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --release --test invariants release_full_list_performance -- --nocapture
```

The tests cover the edit scenarios, hostile input, file boundaries, concurrent writers and determinism across processes. A seeded reference model checks 2,000 sequences of 64 edits, including failed edits. The release performance test measures `apply` plus `to_json` for 500 tasks with 10 subtasks each and checks a median below 5 ms.

Developed with AI assistance (Claude, GPT agents) under the maintainer's review.
