# SchemaForge

SchemaForge is a desktop tool for generating database dictionary documents from database schemas.

The desktop shell is built with Tauri, React, and TypeScript. Document generation is handled by ForgeCore, a Rust generation core that inspects database metadata and writes database dictionary documents.

## Features

- Configure JDBC connection information in the desktop UI
- Generate database dictionary documents for one or more MySQL schemas
- Support HTML, Word, and Markdown output in the Rust ForgeCore path
- Control output language for built-in labels, currently Chinese (`zh-CN`) and English (`en-US`)
- Keep the database inspector behind a Rust trait so additional databases can be added later
- Recognize MySQL, PostgreSQL, and Oracle JDBC URLs; MySQL metadata inspection is implemented first
- Keep configuration in memory for the current window; the app does not persist database credentials by default

## Project Structure

```text
schema-forge/
  src/                  React frontend
  src-tauri/            Tauri desktop shell
  src-tauri/src/forge_core/
                        Rust metadata inspector and document renderer
  src-tauri/src/forge_core/i18n/
                        Built-in document label language files
  config-template/      Example application.yml template
```

## Development

Install frontend dependencies:

```bash
npm install
```

Start the desktop app:

```bash
npm run tauri dev
```

Build checks:

```bash
npm run build
cd src-tauri && cargo check
```

## Configuration Behavior

SchemaForge passes the UI configuration directly to the Rust ForgeCore command. ForgeCore currently supports MySQL metadata inspection and writes generated files to the configured output directory.

Set `output.language` to `zh-CN` or `en-US` to control the generated document labels and table headings. Built-in label files live in `src-tauri/src/forge_core/i18n/`, so future languages can be added with the same JSON structure.

It does not save database credentials or `application.yml` to a persistent file by default.

### Development Connection Drafts

The development-only "remember connection information" control is hidden by default in every build, including `npm run tauri dev`. To enable it on an individual developer machine, create a local `.env` file from `.env.example` and set `VITE_ENABLE_DEV_CONNECTION_STORAGE=true` before starting the app. The draft deliberately excludes the password; production builds always keep this control disabled.

## Ignored Tables

Some operational tables do not need database dictionary pages or ER diagram nodes. Add them under `tables.ignore`:

```yaml
tables:
  ignore:
    - flyway_schema_history
    - databasechangelog
    - databasechangeloglock
    - "*_log"
    - "*_bak"
```

Patterns are case-insensitive and support `*` as a simple wildcard. Ignored tables are removed before rendering documents and ER diagrams; relations pointing to ignored tables are removed as well.

## Relation File Template

Production databases often omit physical foreign keys. To supply business relationships for ER diagrams, create a JSON file like `config-template/schema-forge-relations.json` and set it in the UI relation file field.

The UI relation template action first tries to inspect the configured schemas and export a draft relation file from real foreign keys, existing relation files, and inferred weak relations. If the database cannot be reached, it falls back to the static sample template.

Minimal relation example:

```json
[
  {
    "_comment": "created_by is not a physical FK, but it points to users.id in business logic.",
    "_fieldSource": "source means relation origin; omit it in relation files to use uploaded by default.",
    "_fieldDescription": "description is optional business context for the relation.",
    "sourceTable": "orders",
    "sourceColumn": "created_by",
    "targetTable": "users",
    "targetColumn": "id",
    "relationType": "many-to-one",
    "source": "uploaded",
    "description": "订单创建人"
  }
]
```

Fields:

- `sourceTable`, `sourceColumn`: column that holds the reference.
- `targetTable`, `targetColumn`: table and column being referenced.
- `relationType`: `many-to-one`, `one-to-one`, `one-to-many`, or `many-to-many`.
- `source`: optional relation origin. Relation files normally use `uploaded`; when omitted, SchemaForge defaults it to `uploaded`.
- `description`: optional text shown as relation context.
- `_comment`, `_fieldSource`, `_fieldDescription`: template-only notes. SchemaForge ignores unknown JSON fields.

SchemaForge uses real database foreign keys first, then file-provided relations, then conservative inferred relations. The first relation discovery pass is deterministic and explainable: it scores candidates from table names, column names, key metadata, indexes, type compatibility, and common child-table suffixes.

Project-specific abbreviations should be configured instead of hard-coded:

```yaml
relations:
  aliases:
    archive: [arc, arch]
    element: [elem]
    interface: [itf, iface]
    async: [asyn]
```

Supported weak-relation examples include:

- `user_id -> users.id`
- `dept_code -> departments.code`
- `ecm_interface.interface_id -> ecm_interface_param.interface_id`
- `ecm_arch_elem_sync_itf.interface_id -> ecm_arc_elem_sync_itf_param.interface_id`
- `ecm_archive_element_itf.interface_id -> ecm_arc_elem_asyn_itf_param.interface_id`
- `ecm_archive.archive_id -> ecm_archive_dtl.archive_id`
- `ecm_archive_dtl.element_id -> ecm_archive_element.element_id`
- `ecm_archive_element.element_id -> ecm_archive_element_attachment.element_id`
- `ecm_archive_element_type.element_type_id -> ecm_archive_element_type_tl.element_type_id`

When multiple candidates have the same confidence, SchemaForge skips the relation instead of guessing.

## License

SchemaForge is released under the MIT License. See [LICENSE](LICENSE).

## Key Dependencies

- Tauri
- React
- TypeScript
- Rust
- ForgeCore
- `MySQL`
