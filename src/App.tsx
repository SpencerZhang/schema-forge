import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import appIcon from "./assets/schemaforge-icon-distinctive.png";
import "./App.css";

type FileType = "HTML" | "WORD" | "MD";
type ProduceType = "freemarker" | "velocity";
type AppConfig = {
  spring: {
    datasource: {
      "driver-class-name": string;
      url: string;
      username: string;
      password: string;
    };
  };
  screw: {
    schemas: string[];
    engine: {
      "file-output-dir": string;
      "open-output-dir": boolean;
      "file-type": FileType;
      "produce-type": ProduceType;
      "file-name"?: string;
    };
  };
};

function App() {
  const [jdbcUrl, setJdbcUrl] = useState(
    "jdbc:mysql://127.0.0.1:3306/?useUnicode=true&characterEncoding=utf8&useSSL=false&serverTimezone=Asia/Shanghai",
  );
  const [username, setUsername] = useState("your_username");
  const [password, setPassword] = useState("your_password");
  const [schemas, setSchemas] = useState("your_database");
  const [outputDir, setOutputDir] = useState("/Users/spencerchang/Downloads/");
  const [openOutputDir, setOpenOutputDir] = useState(true);
  const [fileType, setFileType] = useState<FileType>("HTML");
  const [produceType, setProduceType] = useState<ProduceType>("freemarker");
  const [fileName, setFileName] = useState("");
  const [status, setStatus] = useState("配置仅保留在当前窗口");

  const schemaList = useMemo(
    () =>
      schemas
        .split(/\r?\n/)
        .map((schema) => schema.trim())
        .filter(Boolean),
    [schemas],
  );

  const config = useMemo<AppConfig>(() => {
    const engine: AppConfig["screw"]["engine"] = {
      "file-output-dir": outputDir,
      "open-output-dir": openOutputDir,
      "file-type": fileType,
      "produce-type": produceType,
    };
    if (fileName.trim()) {
      engine["file-name"] = fileName.trim();
    }
    return {
      spring: {
        datasource: {
          "driver-class-name": "com.mysql.cj.jdbc.Driver",
          url: jdbcUrl,
          username,
          password,
        },
      },
      screw: {
        schemas: schemaList.length ? schemaList : ["your_database"],
        engine,
      },
    };
  }, [
    fileName,
    fileType,
    jdbcUrl,
    openOutputDir,
    outputDir,
    password,
    produceType,
    schemaList,
    username,
  ]);

  useEffect(() => {
    setStatus("CLI 生成器按需启动，配置不会保存到本地");
  }, []);

  async function generateDoc() {
    setStatus("正在生成文档...");
    try {
      const result = await invoke<{
        schemas: string[];
        output_dir: string;
        stdout: string;
      }>("generate_doc", { config });
      setStatus(`生成完成：${result.schemas.join(", ")} -> ${result.output_dir}`);
    } catch (error) {
      setStatus(`生成失败：${String(error)}`);
    }
  }

  const yaml = useMemo(() => {
    const schemaLines = schemaList
      .map((schema) => `    - ${schema}`)
      .join("\n");
    const fileNameLine = fileName.trim()
      ? `    file-name: ${fileName.trim()}\n`
      : "";

    return `spring:
  datasource:
    driver-class-name: com.mysql.cj.jdbc.Driver
    url: "${jdbcUrl}"
    username: ${username}
    password: ${password ? "******" : ""}

screw:
  schemas:
${schemaLines || "    - your_database"}
  engine:
    file-output-dir: ${outputDir}
    open-output-dir: ${openOutputDir}
    file-type: ${fileType}
    produce-type: ${produceType}
${fileNameLine}`;
  }, [
    fileName,
    fileType,
    jdbcUrl,
    openOutputDir,
    outputDir,
    password,
    produceType,
    schemaList,
    username,
  ]);

  const statusTone =
    status.includes("失败") || status.includes("未启动")
      ? "danger"
      : status.includes("正在")
        ? "busy"
        : "ready";

  return (
    <main className="app-shell">
      <aside className="rail">
        <div className="brand-block">
          <img className="app-icon" src={appIcon} alt="" />
          <div>
            <p className="eyebrow">SchemaForge</p>
            <h1>数据库字典工坊</h1>
          </div>
        </div>

        <nav className="nav-stack">
          <button className="nav-item active" type="button">
            <span>01</span>
            配置源
          </button>
          <button className="nav-item" type="button">
            <span>02</span>
            生成任务
          </button>
          <button className="nav-item" type="button">
            <span>03</span>
            输出记录
          </button>
        </nav>

        <div className="rail-footer">
          <span className={`status-dot ${statusTone}`} />
          <p>{status}</p>
        </div>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div>
            <p className="section-kicker">配置中心</p>
            <h2>配置数据库连接并生成数据字典</h2>
          </div>
          <div className="actions">
            <button className="primary" type="button" onClick={generateDoc}>
              生成文档
            </button>
          </div>
        </header>

        <div className="summary-strip">
          <div>
            <span>{schemaList.length || 1}</span>
            <p>Schema</p>
          </div>
          <div>
            <span>{fileType}</span>
            <p>文件类型</p>
          </div>
          <div>
            <span>{produceType}</span>
            <p>模板引擎</p>
          </div>
          <div>
            <span>{openOutputDir ? "ON" : "OFF"}</span>
            <p>打开目录</p>
          </div>
        </div>

        <div className="content-grid">
          <form className="config-panel">
            <section className="form-section">
              <div className="section-head">
                <span className="section-index">A</span>
                <div>
                  <h3>数据库连接</h3>
                  <p>连接到 MySQL 元数据源</p>
                </div>
              </div>
              <label>
                <span>JDBC URL</span>
                <input
                  value={jdbcUrl}
                  onChange={(event) => setJdbcUrl(event.currentTarget.value)}
                />
              </label>
              <div className="two-column">
                <label>
                  <span>用户名</span>
                  <input
                    value={username}
                    onChange={(event) => setUsername(event.currentTarget.value)}
                  />
                </label>
                <label>
                  <span>密码</span>
                  <input
                    type="password"
                    value={password}
                    onChange={(event) => setPassword(event.currentTarget.value)}
                  />
                </label>
              </div>
            </section>

            <section className="form-section">
              <div className="section-head">
                <span className="section-index">B</span>
                <div>
                  <h3>Schema 范围</h3>
                  <p>每行一个 schema，缺省文件名使用 schema 名称</p>
                </div>
              </div>
              <label>
                <span>Schema 列表</span>
                <textarea
                  rows={5}
                  value={schemas}
                  onChange={(event) => setSchemas(event.currentTarget.value)}
                />
              </label>
            </section>

            <section className="form-section">
              <div className="section-head">
                <span className="section-index">C</span>
                <div>
                  <h3>生成设置</h3>
                  <p>控制输出位置、格式和模板实现</p>
                </div>
              </div>
              <label>
                <span>输出目录</span>
                <input
                  value={outputDir}
                  onChange={(event) => setOutputDir(event.currentTarget.value)}
                />
              </label>
              <div className="three-column">
                <label>
                  <span>文件类型</span>
                  <select
                    value={fileType}
                    onChange={(event) =>
                      setFileType(event.currentTarget.value as FileType)
                    }
                  >
                    <option value="HTML">HTML</option>
                    <option value="WORD">WORD</option>
                    <option value="MD">MD</option>
                  </select>
                </label>
                <label>
                  <span>模板</span>
                  <select
                    value={produceType}
                    onChange={(event) =>
                      setProduceType(event.currentTarget.value as ProduceType)
                    }
                  >
                    <option value="freemarker">freemarker</option>
                    <option value="velocity">velocity</option>
                  </select>
                </label>
                <label>
                  <span>文件名</span>
                  <input
                    placeholder="留空"
                    value={fileName}
                    onChange={(event) => setFileName(event.currentTarget.value)}
                  />
                </label>
              </div>
              <label className="switch-row">
                <input
                  type="checkbox"
                  checked={openOutputDir}
                  onChange={(event) =>
                    setOpenOutputDir(event.currentTarget.checked)
                  }
                />
                <span>生成后打开输出目录</span>
              </label>
            </section>
          </form>

          <aside className="side-panel">
            <section className="preview-panel">
              <div className="preview-header">
                <div>
                  <h3>配置预览</h3>
                  <p>仅用于本次生成，不写入本地文件</p>
                </div>
                <span>YAML</span>
              </div>
              <pre>{yaml}</pre>
            </section>

            <section className="run-panel">
              <div className="run-glow" />
              <h3>生成状态</h3>
              <p>{status}</p>
              <div className="output-path">{outputDir}</div>
            </section>
          </aside>
        </div>
      </section>
    </main>
  );
}

export default App;
