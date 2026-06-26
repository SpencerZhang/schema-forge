# SchemaForge

SchemaForge is a desktop tool for generating database dictionary documents from database schemas.

The desktop shell is built with Tauri, React, and TypeScript. Document generation is delegated to a lightweight Java CLI generator that uses `screw-core` to inspect database metadata and generate documents.

## Features

- Configure JDBC connection information in the desktop UI
- Generate database dictionary documents for one or more schemas
- Support HTML, Word, and Markdown output types provided by `screw-core`
- Keep configuration in memory for the current window; the app does not persist database credentials by default

## Project Structure

```text
schema-forge/
  src/                  React frontend
  src-tauri/            Tauri desktop shell
  backend/              Java CLI generator
  config-template/      Example application.yml template
```

## Development

Install frontend dependencies:

```bash
npm install
```

Build the Java generator:

```bash
npm run generator:build
```

Start the desktop app:

```bash
npm run tauri dev
```

Build checks:

```bash
npm run build
npm run generator:build
```

## Configuration Behavior

SchemaForge currently writes the UI configuration to a temporary file only when a generation task starts, passes that file to the Java CLI generator, and removes the temporary file after the generator exits.

It does not save database credentials or `application.yml` to a persistent file by default.

## Open Source Notice

SchemaForge uses [`screw-core`](https://github.com/pingfangushi/screw), an open-source database table structure documentation generator.

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
- Java CLI generator
- Maven
- `cn.smallbun.screw:screw-core`
- HikariCP
- MySQL Connector/J
