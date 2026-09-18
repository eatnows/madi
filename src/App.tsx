import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type WorktreeInfo = {
  name: string;
  path: string;
  branch: string | null;
  head_oid: string | null;
  is_main: boolean;
};

type Segment = { text: string; emphasized: boolean };

type DiffLineTag = "equal" | "delete" | "insert" | "gap";

type DiffLine = {
  tag: DiffLineTag;
  old_lineno: number | null;
  new_lineno: number | null;
  segments: Segment[];
  skipped: number | null;
};

type FileDiff = {
  path: string;
  status: string;
  additions: number;
  deletions: number;
  section: "committed" | "uncommitted";
  binary: boolean;
  lines: DiffLine[];
};

type DiffResult = {
  merge_base_oid: string;
  head_oid: string;
  files: FileDiff[];
};

type Row = { gap: true } | { gap: false; left: DiffLine | null; right: DiffLine | null };

/** Pairs delete/insert runs side by side so they render as aligned old|new columns. */
function buildRows(lines: DiffLine[]): Row[] {
  const rows: Row[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.tag === "gap") {
      rows.push({ gap: true });
      i++;
      continue;
    }
    if (line.tag === "equal") {
      rows.push({ gap: false, left: line, right: line });
      i++;
      continue;
    }
    const deletes: DiffLine[] = [];
    while (i < lines.length && lines[i].tag === "delete") deletes.push(lines[i++]);
    const inserts: DiffLine[] = [];
    while (i < lines.length && lines[i].tag === "insert") inserts.push(lines[i++]);
    const max = Math.max(deletes.length, inserts.length);
    for (let k = 0; k < max; k++) {
      rows.push({ gap: false, left: deletes[k] ?? null, right: inserts[k] ?? null });
    }
  }
  return rows;
}

function Cell({ line, side }: { line: DiffLine | null; side: "left" | "right" }) {
  const lineno = side === "left" ? line?.old_lineno : line?.new_lineno;
  const linenoClass = line ? `lineno lineno--${line.tag}` : "lineno";
  const tagClass = line ? `diff-cell diff-cell--${line.tag}` : "diff-cell";
  return (
    <>
      <span className={linenoClass}>{lineno ?? ""}</span>
      <span className={tagClass}>
        {line?.segments.map((s, i) => (
          <span key={i} className={s.emphasized ? "diff-emphasis" : undefined}>
            {s.text}
          </span>
        ))}
      </span>
    </>
  );
}

function SideBySideDiff({ file }: { file: FileDiff }) {
  if (file.binary) return <p className="diff-note">Binary file, no preview.</p>;
  const rows = buildRows(file.lines);
  return (
    <div className="diff-grid">
      {rows.map((row, i) =>
        row.gap ? (
          <div className="diff-gap" key={i}>
            ⋯ unchanged ⋯
          </div>
        ) : (
          <div className="diff-row" key={i}>
            <Cell line={row.left} side="left" />
            <Cell line={row.right} side="right" />
          </div>
        ),
      )}
    </div>
  );
}

/** Drag handle between two panels; reports the delta in px while dragging. */
function Resizer({ onResize }: { onResize: (deltaX: number) => void }) {
  const dragging = useRef(false);
  const lastX = useRef(0);

  function onMouseDown(e: React.MouseEvent) {
    dragging.current = true;
    lastX.current = e.clientX;
    const onMouseMove = (ev: MouseEvent) => {
      if (!dragging.current) return;
      onResize(ev.clientX - lastX.current);
      lastX.current = ev.clientX;
    };
    const onMouseUp = () => {
      dragging.current = false;
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  }

  return <div className="resizer" onMouseDown={onMouseDown} />;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function App() {
  const [repoPath, setRepoPath] = useState("");
  const [baseBranch, setBaseBranch] = useState("main");
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [selectedWorktree, setSelectedWorktree] = useState<WorktreeInfo | null>(null);
  const [diff, setDiff] = useState<DiffResult | null>(null);
  const [selectedFile, setSelectedFile] = useState<FileDiff | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [worktreePanelWidth, setWorktreePanelWidth] = useState(248);
  const [filePanelWidth, setFilePanelWidth] = useState(260);

  async function loadWorktrees() {
    setError(null);
    setDiff(null);
    setSelectedWorktree(null);
    setSelectedFile(null);
    try {
      setWorktrees(await invoke<WorktreeInfo[]>("list_worktrees", { repoPath }));
    } catch (e) {
      setWorktrees([]);
      setError(String(e));
    }
  }

  async function selectWorktree(wt: WorktreeInfo) {
    setSelectedWorktree(wt);
    setDiff(null);
    setSelectedFile(null);
    setError(null);
    try {
      const result = await invoke<DiffResult>("diff_against_base", {
        worktreePath: wt.path,
        baseBranch,
      });
      setDiff(result);
      setSelectedFile(result.files[0] ?? null);
    } catch (e) {
      setError(String(e));
    }
  }

  const projectName = repoPath.split("/").filter(Boolean).pop() ?? "";
  const committedFiles = diff?.files.filter((f) => f.section === "committed") ?? [];
  const uncommittedFiles = diff?.files.filter((f) => f.section === "uncommitted") ?? [];

  return (
    <div className="app-shell">
      <div className="topbar">
        <form
          className="topbar-form"
          onSubmit={(e) => {
            e.preventDefault();
            loadWorktrees();
          }}
        >
          <input
            value={repoPath}
            onChange={(e) => setRepoPath(e.currentTarget.value)}
            placeholder="/path/to/repo"
          />
          <input
            value={baseBranch}
            onChange={(e) => setBaseBranch(e.currentTarget.value)}
            placeholder="base branch"
            className="topbar-branch"
          />
          <button type="submit">Rescan</button>
        </form>
        {error && <span className="topbar-error">{error}</span>}
      </div>

      <div className="main-row">
        <div className="rail">
          {projectName && <div className="rail-tile rail-tile--active">{projectName[0]?.toUpperCase()}</div>}
          <div className="rail-tile rail-tile--add" title="Multiple projects: not wired up yet">
            +
          </div>
        </div>

        <div className="panel worktree-panel" style={{ width: worktreePanelWidth }}>
          <div className="panel-header">
            <div className="panel-title">{projectName || "No project"}</div>
            <div className="panel-subtitle">Worktrees</div>
          </div>
          <div className="panel-body">
            {worktrees.map((wt) => (
              <div
                key={wt.path}
                className={
                  "list-row" + (selectedWorktree?.path === wt.path ? " list-row--selected" : "")
                }
                onClick={() => selectWorktree(wt)}
              >
                <div className="list-row-title">{wt.name}</div>
                <div className="list-row-sub">{wt.branch ?? "(detached)"}</div>
              </div>
            ))}
          </div>
        </div>

        <Resizer onResize={(dx) => setWorktreePanelWidth((w) => clamp(w + dx, 180, 420))} />

        <div className="panel file-panel" style={{ width: filePanelWidth }}>
          {selectedWorktree && diff && (
            <>
              <div className="panel-header">
                <div className="panel-title">{selectedWorktree.name}</div>
                <div className="panel-subtitle">vs {baseBranch}</div>
              </div>
              <div className="panel-body">
                {committedFiles.length > 0 && (
                  <div className="file-section-label">COMMITTED · {committedFiles.length}</div>
                )}
                {committedFiles.map((f) => (
                  <FileRow key={f.path} file={f} selected={selectedFile === f} onClick={() => setSelectedFile(f)} />
                ))}
                {uncommittedFiles.length > 0 && (
                  <div className="file-section-label">UNCOMMITTED · {uncommittedFiles.length}</div>
                )}
                {uncommittedFiles.map((f) => (
                  <FileRow key={f.path} file={f} selected={selectedFile === f} onClick={() => setSelectedFile(f)} />
                ))}
                {diff.files.length === 0 && <p className="diff-note">No differences.</p>}
              </div>
            </>
          )}
        </div>

        <Resizer onResize={(dx) => setFilePanelWidth((w) => clamp(w + dx, 180, 480))} />

        <div className="panel diff-panel">
          {selectedFile && (
            <>
              <div className="diff-panel-header">
                <span className="diff-panel-path">{selectedFile.path}</span>
                <span className="num diff-add">+{selectedFile.additions}</span>
                <span className="num diff-del">-{selectedFile.deletions}</span>
              </div>
              <div className="diff-panel-body">
                <SideBySideDiff file={selectedFile} />
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function FileRow({ file, selected, onClick }: { file: FileDiff; selected: boolean; onClick: () => void }) {
  const statusLetter = file.status[0]?.toUpperCase() ?? "?";
  return (
    <div className={"file-row" + (selected ? " file-row--selected" : "")} onClick={onClick}>
      <span className={`file-status file-status--${file.status}`}>{statusLetter}</span>
      <span className="file-path">{file.path}</span>
      <span className="num diff-add">+{file.additions}</span>
      <span className="num diff-del">-{file.deletions}</span>
    </div>
  );
}

export default App;
