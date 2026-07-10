import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import appIcon from "./assets/schemaforge-icon-distinctive.png";
import "./App.css";

type FileType = "HTML" | "WORD" | "MD";
type OutputLanguage = "zh-CN" | "en-US";
type AppConfig = {
  database: {
    driver: string;
    url: string;
    username: string;
    password: string;
  };
  schemas: string[];
  tables?: {
    ignore: string[];
  };
  output: {
    dir: string;
    "open-dir": boolean;
    "file-type": FileType;
    language: OutputLanguage;
    "file-name"?: string;
  };
  relations?: {
    file: string;
    aliases?: Record<string, string[]>;
  };
};

type DevConnectionDraft = {
  jdbcUrl: string;
  username: string;
  schemas: string;
  ignoredTables: string;
  outputDir: string;
  relationFile: string;
};

const DEV_CONNECTION_STORAGE_KEY = "schema-forge-dev-connection";
// Deliberately opt-in: development builds should behave like production by default.
const isDevMode =
  import.meta.env.DEV &&
  import.meta.env.VITE_ENABLE_DEV_CONNECTION_STORAGE === "true";
const RELATION_TEMPLATE_FILE_NAME = "schema-forge-relations.json";
const RELATION_TEMPLATE = [
  {
    _comment: "示例 1：created_by 不是物理外键，但业务上指向 users.id。",
    _fieldSource: "source 表示这条关系的来源；关系文件里可省略，默认 uploaded。",
    _fieldDescription: "description 是业务说明，可省略；会作为 ER 图关系上下文使用。",
    sourceTable: "orders",
    sourceColumn: "created_by",
    targetTable: "users",
    targetColumn: "id",
    relationType: "many-to-one",
    source: "uploaded",
    description: "订单创建人",
  },
  {
    _comment: "示例 2：同一张来源表可以维护多条不同业务关系。",
    sourceTable: "orders",
    sourceColumn: "approved_by",
    targetTable: "users",
    targetColumn: "id",
    relationType: "many-to-one",
    source: "uploaded",
    description: "订单审批人",
  },
  {
    _comment: "示例 3：也支持非 id 字段，只要能表达明确业务关联。",
    sourceTable: "payments",
    sourceColumn: "order_no",
    targetTable: "orders",
    targetColumn: "order_no",
    relationType: "many-to-one",
    source: "uploaded",
    description: "支付单对应订单",
  },
];

function App() {
  const [jdbcUrl, setJdbcUrl] = useState(
    "jdbc:mysql://127.0.0.1:3306/?useUnicode=true&characterEncoding=utf8&useSSL=false&serverTimezone=Asia/Shanghai",
  );
  const [username, setUsername] = useState("your_username");
  const [password, setPassword] = useState("your_password");
  const [schemas, setSchemas] = useState("your_database");
  const [ignoredTables, setIgnoredTables] = useState(
    "flyway_schema_history\ndatabasechangelog\ndatabasechangeloglock\n*_log\n*_bak",
  );
  const [outputDir, setOutputDir] = useState("./schema-forge-output");
  const [openOutputDir, setOpenOutputDir] = useState(true);
  const [fileType, setFileType] = useState<FileType>("HTML");
  const [language, setLanguage] = useState<OutputLanguage>("zh-CN");
  const [fileName, setFileName] = useState("");
  const [relationFile, setRelationFile] = useState("");
  const [isGenerating, setIsGenerating] = useState(false);
  const [isGeneratingEr, setIsGeneratingEr] = useState(false);
  const [runMessage, setRunMessage] = useState("");
  const [rememberDevConnection, setRememberDevConnection] = useState(false);

  useEffect(() => {
    if (!isDevMode) {
      return;
    }
    const saved = window.localStorage.getItem(DEV_CONNECTION_STORAGE_KEY);
    if (!saved) {
      return;
    }
    try {
      const draft = JSON.parse(saved) as Partial<DevConnectionDraft>;
      if (typeof draft.jdbcUrl === "string") setJdbcUrl(draft.jdbcUrl);
      if (typeof draft.username === "string") setUsername(draft.username);
      if (typeof draft.schemas === "string") setSchemas(draft.schemas);
      if (typeof draft.ignoredTables === "string") setIgnoredTables(draft.ignoredTables);
      if (typeof draft.outputDir === "string") setOutputDir(draft.outputDir);
      if (typeof draft.relationFile === "string") setRelationFile(draft.relationFile);
      setRememberDevConnection(true);
    } catch {
      window.localStorage.removeItem(DEV_CONNECTION_STORAGE_KEY);
    }
  }, []);

  useEffect(() => {
    if (!isDevMode) {
      return;
    }
    if (!rememberDevConnection) {
      window.localStorage.removeItem(DEV_CONNECTION_STORAGE_KEY);
      return;
    }
    const draft: DevConnectionDraft = {
      jdbcUrl,
      username,
      schemas,
      ignoredTables,
      outputDir,
      relationFile,
    };
    window.localStorage.setItem(DEV_CONNECTION_STORAGE_KEY, JSON.stringify(draft));
  }, [ignoredTables, jdbcUrl, outputDir, relationFile, rememberDevConnection, schemas, username]);

  const schemaList = useMemo(
    () =>
      schemas
        .split(/\r?\n/)
        .map((schema) => schema.trim())
        .filter(Boolean),
    [schemas],
  );

  const ignoredTableList = useMemo(
    () =>
      ignoredTables
        .split(/\r?\n/)
        .map((table) => table.trim())
        .filter(Boolean),
    [ignoredTables],
  );

  const config = useMemo<AppConfig>(() => {
    const output: AppConfig["output"] = {
      dir: outputDir,
      "open-dir": openOutputDir,
      "file-type": fileType,
      language,
    };
    if (fileName.trim()) {
      output["file-name"] = fileName.trim();
    }
    const nextConfig: AppConfig = {
      database: {
        driver: "mysql",
        url: jdbcUrl,
        username,
        password,
      },
      schemas: schemaList.length ? schemaList : ["your_database"],
      output,
    };
    if (ignoredTableList.length) {
      nextConfig.tables = { ignore: ignoredTableList };
    }
    if (relationFile.trim()) {
      nextConfig.relations = { file: relationFile.trim() };
    }
    return nextConfig;
  }, [
    fileName,
    fileType,
    jdbcUrl,
    language,
    openOutputDir,
    outputDir,
    password,
    ignoredTableList,
    relationFile,
    schemaList,
    username,
  ]);

  async function generateDoc() {
    if (isGenerating || isGeneratingEr) {
      return;
    }
    setIsGenerating(true);
    setRunMessage("正在生成文档...");
    try {
      const result = await invoke<{
        schemas: string[];
        output_dir: string;
        stdout: string;
      }>("generate_doc", { config });
      setRunMessage(`生成完成：${result.schemas.join(", ")}`);
    } catch (error) {
      setRunMessage(`生成失败：${String(error)}`);
    } finally {
      setIsGenerating(false);
    }
  }

  async function generateErDiagram() {
    if (isGenerating || isGeneratingEr) {
      return;
    }
    setIsGeneratingEr(true);
    setRunMessage("正在生成 ER 图...");
    try {
      const result = await invoke<{
        schemas: string[];
        output_dir: string;
        stdout: string;
      }>("generate_er_diagram", { config });
      setRunMessage(`ER 图生成完成：${result.schemas.join(", ")}`);
    } catch (error) {
      setRunMessage(`ER 图生成失败：${String(error)}`);
    } finally {
      setIsGeneratingEr(false);
    }
  }

  function downloadJsonFile(fileName: string, content: string) {
    const blob = new Blob([content], {
      type: "application/json;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = fileName;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
  }

  async function downloadRelationTemplate() {
    if (isGenerating || isGeneratingEr) {
      return;
    }
    setRunMessage("正在生成关系模板...");
    try {
      const result = await invoke<{
        file_name: string;
        content: string;
        relation_count: number;
      }>("generate_relation_template", { config });
      downloadJsonFile(result.file_name, result.content);
      setRunMessage(`关系模板已生成：${result.relation_count} 条关系`);
    } catch (error) {
      downloadJsonFile(
        RELATION_TEMPLATE_FILE_NAME,
        `${JSON.stringify(RELATION_TEMPLATE, null, 2)}\n`,
      );
      setRunMessage(`未能读取数据库，已下载静态关系模板：${String(error)}`);
    }
  }

  return (
    <main className="app-shell">
      <section className="workspace">
        <header className="topbar">
          <div className="title-block">
            <img className="app-icon" src={appIcon} alt="" />
            <div>
              <p className="section-kicker">SchemaForge</p>
              <h2>数据库文档与 ER 图生成器</h2>
            </div>
          </div>
          <div className="actions">
            <button
              className="action-button primary"
              type="button"
              disabled={isGenerating || isGeneratingEr}
              aria-busy={isGenerating}
              onClick={generateDoc}
            >
              {isGenerating && <span className="spinner" aria-hidden="true" />}
              {isGenerating ? "生成中" : "生成文档"}
            </button>
            <button
              className="action-button secondary"
              type="button"
              disabled={isGenerating || isGeneratingEr}
              aria-busy={isGeneratingEr}
              onClick={generateErDiagram}
            >
              {isGeneratingEr && <span className="spinner dark" aria-hidden="true" />}
              {isGeneratingEr ? "生成中" : "生成 ER 图"}
            </button>
          </div>
        </header>

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
              {isDevMode && (
                <label className="switch-row dev-switch">
                  <input
                    type="checkbox"
                    checked={rememberDevConnection}
                    onChange={(event) =>
                      setRememberDevConnection(event.currentTarget.checked)
                    }
                  />
                  <span>开发调试时记住连接信息</span>
                </label>
              )}
            </section>

            <section className="form-section">
              <div className="section-head">
                <span className="section-index">B</span>
                <div>
                  <h3>Schema 范围</h3>
                  <p>每行一个 schema，缺省文件名使用 schema 名称</p>
                </div>
              </div>
              <div className="scope-grid">
                <label>
                  <span>Schema 列表</span>
                  <textarea
                    rows={3}
                    value={schemas}
                    autoCapitalize="none"
                    autoCorrect="off"
                    spellCheck={false}
                    onChange={(event) => setSchemas(event.currentTarget.value)}
                  />
                </label>
                <label>
                  <span>忽略表</span>
                  <textarea
                    rows={3}
                    placeholder="每行一个表名，支持 tmp_*、*_log"
                    value={ignoredTables}
                    autoCapitalize="none"
                    autoCorrect="off"
                    spellCheck={false}
                    onChange={(event) => setIgnoredTables(event.currentTarget.value)}
                  />
                </label>
              </div>
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
              <div className="settings-grid">
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
                  <span>输出语言</span>
                  <select
                    value={language}
                    onChange={(event) =>
                      setLanguage(event.currentTarget.value as OutputLanguage)
                    }
                  >
                    <option value="zh-CN">中文</option>
                    <option value="en-US">English</option>
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
              <label>
                <span className="label-row">
                  关系文件
                  <button
                    className="inline-action"
                    type="button"
                    onClick={downloadRelationTemplate}
                  >
                    生成模板
                  </button>
                </span>
                <input
                  placeholder="./schema-forge-relations.json"
                  value={relationFile}
                  onChange={(event) => setRelationFile(event.currentTarget.value)}
                />
              </label>
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
            <section className="task-panel">
              <h3>执行状态</h3>
              <div className="status-box">
                <span
                  className={
                    runMessage.includes("失败")
                      ? "status-dot danger"
                      : runMessage
                        ? "status-dot success"
                        : "status-dot"
                  }
                />
                <strong>{runMessage || "等待生成任务"}</strong>
              </div>
            </section>

            <section className="task-panel">
              <h3>任务摘要</h3>
              <div className="task-list">
                <div>
                  <span>Schema</span>
                  <strong>{schemaList.join(", ") || "your_database"}</strong>
                </div>
                <div>
                  <span>输出格式</span>
                  <strong>{fileType}</strong>
                </div>
                <div>
                  <span>输出语言</span>
                  <strong>{language === "zh-CN" ? "中文" : "English"}</strong>
                </div>
                <div>
                  <span>文件名</span>
                  <strong>{fileName.trim() || "默认使用 Schema 名称"}</strong>
                </div>
                <div>
                  <span>关系文件</span>
                  <strong>{relationFile.trim() || "未配置"}</strong>
                </div>
                <div>
                  <span>忽略表</span>
                  <strong>{ignoredTableList.length ? `${ignoredTableList.length} 项` : "未配置"}</strong>
                </div>
              </div>
            </section>

            <section className="task-panel">
              <h3>输出目标</h3>
              <div className="target-list">
                <div>
                  <span>输出目录</span>
                  <strong>{outputDir}</strong>
                </div>
                <div>
                  <span>文档文件</span>
                  <strong>
                    {(fileName.trim() || schemaList[0] || "your_database")}.{fileType.toLowerCase()}
                  </strong>
                </div>
                <div>
                  <span>ER 图文件</span>
                  <strong>{schemaList[0] || "your_database"}-er.html</strong>
                </div>
                <div>
                  <span>关系来源</span>
                  <strong>{relationFile.trim() ? "数据库外键 / 推断 / 关系文件" : "数据库外键 / 推断"}</strong>
                </div>
              </div>
            </section>
          </aside>
        </div>
      </section>
    </main>
  );
}

export default App;
