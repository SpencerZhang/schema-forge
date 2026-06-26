# SchemaForge

SchemaForge is a desktop tool for generating database dictionary documents from database schemas.

The desktop shell is built with Tauri, React, and TypeScript. Document generation is handled by ForgeCore, a Rust generation core that inspects database metadata and writes database dictionary documents.

## Features

- Configure JDBC connection information in the desktop UI
- Generate database dictionary documents for one or more MySQL schemas
- Support HTML and Markdown output in the Rust ForgeCore path
- Keep the database inspector behind a Rust trait so additional databases can be added later
- Keep configuration in memory for the current window; the app does not persist database credentials by default

## Project Structure

```text
schema-forge/
  src/                  React frontend
  src-tauri/            Tauri desktop shell
  src-tauri/src/forge_core/
                        Rust metadata inspector and document renderer
  backend/              Legacy Java CLI generator kept during migration
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

It does not save database credentials or `application.yml` to a persistent file by default.

## Open Source Notice

The legacy Java generator in `backend/` uses [`screw-core`](https://github.com/pingfangushi/screw), an open-source database table structure documentation generator.

The Maven metadata for `cn.smallbun.screw:screw-core:1.0.5` declares the parent project license as:

```text
GNU Lesser General Public License v3.0
```

License URL:

```text
https://www.gnu.org/licenses/lgpl-3.0.html
```

When distributing SchemaForge, make sure to comply with the LGPL-3.0 obligations for the `screw-core` dependency and its notices. SchemaForge is not affiliated with or endorsed by the `screw` project.

## License

SchemaForge is released under the MIT License. See [LICENSE](LICENSE).

## Key Dependencies

- Tauri
- React
- TypeScript
- Rust
- ForgeCore
- `mysql`
