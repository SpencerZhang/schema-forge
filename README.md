# SchemaForge

SchemaForge is a desktop tool for generating database dictionary documents from database schemas.

The desktop shell is built with Tauri, React, and TypeScript. The local backend is a Spring Boot service that uses `screw-core` to inspect database metadata and generate documents.

## Features

- Configure JDBC connection information in the desktop UI
- Generate database dictionary documents for one or more schemas
- Support HTML, Word, and Markdown output types provided by `screw-core`
- Preview the generated YAML-style configuration before running
- Keep configuration in memory for the current window; the app does not persist database credentials by default

## Project Structure

```text
schema-forge/
  src/                  React frontend
  src-tauri/            Tauri desktop shell
  backend/              Spring Boot local backend
  config-template/      Example application.yml template
```

## Development

Install frontend dependencies:

```bash
npm install
```

Start the backend:

```bash
npm run backend:dev
```

Start the desktop app:

```bash
npm run tauri dev
```

Build checks:

```bash
npm run build
npm run backend:build
```

## Configuration Behavior

SchemaForge currently sends the UI configuration directly to the local backend when generating documents.

It does not save database credentials or `application.yml` to disk by default.

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

## Key Dependencies

- Tauri
- React
- TypeScript
- Spring Boot
- `cn.smallbun.screw:screw-core`
- HikariCP
- MySQL Connector/J
