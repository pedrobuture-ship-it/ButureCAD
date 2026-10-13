# Project format (schema 1)

A project is a folder:

```text
MyBoard/
  project.buture      manifest (JSON, deterministic, pretty-printed, trailing newline)
  schematic/ pcb/ libraries/ models/ scripts/ rules/ output/
  .buture/            local state, add to .gitignore: backups/, autosave/, session.lock
```

## `project.buture`

```json
{
  "format": "buturecad.project",
  "schema_version": 1,
  "name": "MyBoard"
}
```

- Unknown fields are rejected in the current schema; extend by bumping `schema_version`.
- No timestamps or machine-specific data are written, so output is stable under Git.
- Files with a newer `schema_version` are refused with an explicit error. Older versions are
  upgraded in memory through the ordered migration chain (`buturecad_io::format::Migrations`).
  Schema 1 is the first version, so the built-in chain is currently empty; the mechanism is
  covered by tests with a synthetic registry.

## Safety

- All writes are atomic: temp file in the same directory, fsync, rename, fsync of the directory.
- `save` keeps up to 5 rotating backups in `.buture/backups/` (only of manifests that parse).
- `autosave` writes `.buture/autosave/project.buture` and never touches the manifest.
- A `.buture/session.lock` exists while a session is open. If it is found on open, the previous
  session crashed; a readable autosave is then offered as recovery. A successful save clears it.
- Malformed files produce errors (`IoError`), never panics.

## Not implemented

- Detection of two live instances opening the same project.
- Schematic/PCB document files (later stages); only the manifest is persisted now.
