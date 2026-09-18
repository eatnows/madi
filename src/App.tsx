import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

type WorktreeInfo = {
  name: string;
  path: string;
  branch: string | null;
  head_oid: string | null;
  is_main: boolean;
  ahead: number | null;
  behind: number | null;
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

type BranchTreeNode = {
  segment: string;
  fullPath: string;
  isBranch: boolean;
  children: Map<string, BranchTreeNode>;
};

/** Groups branch names sharing a "/" prefix (e.g. "feature/x", "feature/y") into a tree. */
function buildBranchTree(branches: string[]): BranchTreeNode {
  const root: BranchTreeNode = { segment: "", fullPath: "", isBranch: false, children: new Map() };
  for (const branch of branches) {
    let node = root;
    let path = "";
    for (const part of branch.split("/")) {
      path = path ? `${path}/${part}` : part;
      let next = node.children.get(part);
      if (!next) {
        next = { segment: part, fullPath: path, isBranch: false, children: new Map() };
        node.children.set(part, next);
      }
      node = next;
    }
    node.isBranch = true;
  }
  return root;
}

function BranchTreeOptions({
  node,
  depth,
  value,
  onPick,
}: {
  node: BranchTreeNode;
  depth: number;
  value: string;
  onPick: (branch: string) => void;
}) {
  const children = [...node.children.values()].sort((a, b) => a.segment.localeCompare(b.segment));
  return (
    <>
      {children.map((child) => (
        <div key={child.fullPath}>
          {child.children.size > 0 ? (
            <div className="searchable-select-folder" style={{ paddingLeft: 8 + depth * 12 }}>
              {child.segment}/
            </div>
          ) : (
            <BranchOption fullPath={child.fullPath} depth={depth} value={value} onPick={onPick} />
          )}
          {child.children.size > 0 && (
            <BranchTreeOptions node={child} depth={depth + 1} value={value} onPick={onPick} />
          )}
          {child.children.size > 0 && child.isBranch && (
            <BranchOption fullPath={child.fullPath} depth={depth + 1} value={value} onPick={onPick} />
          )}
        </div>
      ))}
    </>
  );
}

function BranchOption({
  fullPath,
  depth,
  value,
  onPick,
}: {
  fullPath: string;
  depth: number;
  value: string;
  onPick: (branch: string) => void;
}) {
  const label = fullPath.split("/").pop();
  return (
    <div
      className={"searchable-select-option" + (fullPath === value ? " searchable-select-option--selected" : "")}
      style={{ paddingLeft: 8 + depth * 12 }}
      onClick={() => onPick(fullPath)}
    >
      {label}
    </div>
  );
}

/** A branch picker with search-to-filter and "/"-prefix folder grouping — a native <select>
 * can't scroll-search or group, and this repo's branch lists can get long. */
function BranchPicker({
  value,
  branches,
  onChange,
}: {
  value: string;
  branches: string[];
  onChange: (branch: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onDocMouseDown(e: MouseEvent) {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", onDocMouseDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onDocMouseDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  function pick(branch: string) {
    onChange(branch);
    setOpen(false);
  }

  const filtered = query ? branches.filter((b) => b.toLowerCase().includes(query.toLowerCase())) : null;

  return (
    <div className="searchable-select" ref={rootRef}>
      <button
        type="button"
        className={"searchable-select-trigger" + (open ? " searchable-select-trigger--open" : "")}
        onClick={() => {
          setOpen((o) => !o);
          setQuery("");
        }}
      >
        <span className="searchable-select-value">{value}</span>
        <ChevronDownIcon />
      </button>
      {open && (
        <div className="searchable-select-popover">
          <input
            className="searchable-select-search"
            autoFocus
            placeholder="Search branches…"
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && filtered && filtered.length > 0) pick(filtered[0]);
            }}
          />
          <div className="searchable-select-list">
            {filtered ? (
              filtered.length === 0 ? (
                <div className="searchable-select-empty">No matches</div>
              ) : (
                filtered.map((b) => <BranchOption key={b} fullPath={b} depth={0} value={value} onPick={pick} />)
              )
            ) : (
              <BranchTreeOptions node={buildBranchTree(branches)} depth={0} value={value} onPick={pick} />
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function RefreshIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9" />
      <path d="M13.5 2.5v3.5H10" />
    </svg>
  );
}

function LayoutSidebarIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round">
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
      <path d="M5.5 2.5v11" strokeLinecap="round" />
    </svg>
  );
}

function LayoutFocusedIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round">
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
    </svg>
  );
}

function ChevronDownIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 6l4 4 4-4" />
    </svg>
  );
}

function defaultBranchOf(branches: string[]): string {
  return branches.includes("main") ? "main" : (branches[0] ?? "main");
}

function baseBranchesStorageKey(repoPath: string) {
  return `worktree-viewer:base-branches:${repoPath}`;
}

function loadPinnedBaseBranches(repoPath: string): Record<string, string> {
  try {
    const raw = localStorage.getItem(baseBranchesStorageKey(repoPath));
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function savePinnedBaseBranches(repoPath: string, map: Record<string, string>) {
  try {
    localStorage.setItem(baseBranchesStorageKey(repoPath), JSON.stringify(map));
  } catch {
    // best-effort; private browsing or storage quota issues just mean re-pinning next launch
  }
}

type ViewMode = "sidebar" | "focused";

function App() {
  const [repoPath, setRepoPath] = useState("");
  const [branches, setBranches] = useState<string[]>([]);
  // Each worktree's base branch is pinned once (at first scan) and remembered here, keyed by
  // worktree path, instead of one global selector that would redefine "changed" for every
  // worktree whenever it's touched.
  const [baseBranches, setBaseBranches] = useState<Record<string, string>>({});
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [selectedWorktree, setSelectedWorktree] = useState<WorktreeInfo | null>(null);
  const [diff, setDiff] = useState<DiffResult | null>(null);
  const [selectedFile, setSelectedFile] = useState<FileDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>("sidebar");
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; worktree: WorktreeInfo } | null>(null);

  const [worktreePanelWidth, setWorktreePanelWidth] = useState(248);
  const [filePanelWidth, setFilePanelWidth] = useState(260);

  async function refreshWorktrees(path: string, pins: Record<string, string>) {
    const wts = await invoke<WorktreeInfo[]>("list_worktrees", { repoPath: path, baseBranches: pins });
    setWorktrees(wts);
    return wts;
  }

  async function scan(path: string) {
    setRepoPath(path);
    setError(null);
    setDiff(null);
    setSelectedWorktree(null);
    setSelectedFile(null);
    try {
      const brs = await invoke<string[]>("list_branches", { repoPath: path });
      setBranches(brs);

      const pins = loadPinnedBaseBranches(path);
      let wts = await refreshWorktrees(path, pins);

      // Pin any newly discovered worktree to a sensible default, then refresh once more so
      // its ahead/behind reflects that pin immediately.
      let changed = false;
      for (const wt of wts) {
        if (!pins[wt.path]) {
          pins[wt.path] = defaultBranchOf(brs);
          changed = true;
        }
      }
      if (changed) wts = await refreshWorktrees(path, pins);

      setBaseBranches(pins);
      savePinnedBaseBranches(path, pins);
    } catch (e) {
      setWorktrees([]);
      setBranches([]);
      setError(String(e));
    }
  }

  async function pickProject() {
    const dir = await open({ directory: true, multiple: false, title: "Open a git repository" });
    if (typeof dir === "string") scan(dir);
  }

  function rescan() {
    if (repoPath) scan(repoPath);
  }

  async function loadDiff(wt: WorktreeInfo, branch: string) {
    setDiff(null);
    setSelectedFile(null);
    setError(null);
    try {
      const result = await invoke<DiffResult>("diff_against_base", {
        worktreePath: wt.path,
        baseBranch: branch,
      });
      setDiff(result);
      setSelectedFile(result.files[0] ?? null);
    } catch (e) {
      setError(String(e));
    }
  }

  function selectWorktree(wt: WorktreeInfo) {
    setSelectedWorktree(wt);
    const base = baseBranches[wt.path];
    if (base) loadDiff(wt, base);
  }

  async function changeWorktreeBase(wt: WorktreeInfo, branch: string) {
    const pins = { ...baseBranches, [wt.path]: branch };
    setBaseBranches(pins);
    savePinnedBaseBranches(repoPath, pins);
    setError(null);
    try {
      const wts = await refreshWorktrees(repoPath, pins);
      if (selectedWorktree?.path === wt.path) {
        const updated = wts.find((w) => w.path === wt.path) ?? wt;
        setSelectedWorktree(updated);
        loadDiff(updated, branch);
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function removeWorktree(wt: WorktreeInfo) {
    const ok = window.confirm(
      `Remove worktree "${wt.name}" (${wt.branch ?? "detached"})?\n\nThis deletes its working directory. Uncommitted changes will be lost.`,
    );
    if (!ok) return;
    setError(null);
    try {
      await invoke("remove_worktree", { repoPath, worktreePath: wt.path });
      if (selectedWorktree?.path === wt.path) {
        setSelectedWorktree(null);
        setDiff(null);
        setSelectedFile(null);
      }
      const pins = Object.fromEntries(Object.entries(baseBranches).filter(([path]) => path !== wt.path));
      setBaseBranches(pins);
      savePinnedBaseBranches(repoPath, pins);
      await refreshWorktrees(repoPath, pins);
    } catch (e) {
      setError(String(e));
    }
  }

  const projectName = repoPath.split("/").filter(Boolean).pop() ?? "";
  const committedFiles = diff?.files.filter((f) => f.section === "committed") ?? [];
  const uncommittedFiles = diff?.files.filter((f) => f.section === "uncommitted") ?? [];
  const selectedBase = selectedWorktree ? baseBranches[selectedWorktree.path] : undefined;

  // Merges panels into a single centered message instead of several empty boxes.
  const noProject = !repoPath;
  const noWorktrees = repoPath !== "" && worktrees.length === 0;
  const noSelection = !noProject && !noWorktrees && !selectedWorktree;
  const noChanges = !!selectedWorktree && !!diff && diff.files.length === 0;
  const wholeAreaEmptyMessage = noProject ? "No project" : noWorktrees ? "No data" : null;
  const detailAreaEmptyMessage = noSelection ? "No data" : noChanges ? "No changes" : null;

  return (
    <div className="app-shell">
      <div className="topbar">
        <div className="breadcrumb">
          <span className="breadcrumb-brand">worktree-viewer</span>
          {projectName && (
            <>
              <span className="breadcrumb-sep">/</span>
              <span>{projectName}</span>
            </>
          )}
          {selectedWorktree && (
            <>
              <span className="breadcrumb-sep">/</span>
              <span>{selectedWorktree.branch ?? selectedWorktree.name}</span>
            </>
          )}
        </div>
        <div className="topbar-right">
          {repoPath && (
            <button type="button" className="icon-btn" onClick={rescan} title="Rescan" aria-label="Rescan">
              <RefreshIcon />
            </button>
          )}
          <div className="view-toggle">
            <button
              type="button"
              className={"view-toggle-btn" + (viewMode === "sidebar" ? " view-toggle-btn--active" : "")}
              onClick={() => setViewMode("sidebar")}
              title="Sidebar layout"
              aria-label="Sidebar layout"
            >
              <LayoutSidebarIcon />
            </button>
            <button
              type="button"
              className={"view-toggle-btn" + (viewMode === "focused" ? " view-toggle-btn--active" : "")}
              onClick={() => setViewMode("focused")}
              title="Focused layout"
              aria-label="Focused layout"
            >
              <LayoutFocusedIcon />
            </button>
          </div>
        </div>
        {error && <span className="topbar-error">{error}</span>}
      </div>

      {viewMode === "sidebar" ? (
        <div className="main-row">
          <div className="rail">
            {projectName && <div className="rail-tile rail-tile--active">{projectName[0]?.toUpperCase()}</div>}
            <button
              type="button"
              className="rail-tile rail-tile--add"
              onClick={pickProject}
              title="Open a git repository"
            >
              +
            </button>
          </div>

          {wholeAreaEmptyMessage ? (
            <EmptyArea message={wholeAreaEmptyMessage} />
          ) : (
            <>
              <div className="panel worktree-panel" style={{ width: worktreePanelWidth }}>
                <div className="panel-header">
                  <div className="panel-title">{projectName}</div>
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
                      onContextMenu={(e) => {
                        e.preventDefault();
                        if (!wt.is_main) setContextMenu({ x: e.clientX, y: e.clientY, worktree: wt });
                      }}
                    >
                      <div className="list-row-title">{wt.name}</div>
                      <div className="list-row-sub">{wt.branch ?? "(detached)"}</div>
                      <div className="list-row-base" onClick={(e) => e.stopPropagation()}>
                        <span className="list-row-base-label">base:</span>
                        <BranchPicker
                          value={baseBranches[wt.path] ?? ""}
                          branches={branches}
                          onChange={(b) => changeWorktreeBase(wt, b)}
                        />
                        {wt.ahead !== null && wt.behind !== null && (
                          <span className="list-row-status">
                            {wt.ahead === 0 && wt.behind === 0 ? "up to date" : `↑${wt.ahead} ↓${wt.behind}`}
                          </span>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </div>

              <Resizer onResize={(dx) => setWorktreePanelWidth((w) => clamp(w + dx, 180, 420))} />

              {detailAreaEmptyMessage ? (
                <EmptyArea message={detailAreaEmptyMessage} />
              ) : (
                <>
                  <div className="panel file-panel" style={{ width: filePanelWidth }}>
                    <div className="panel-header">
                      <div className="panel-title">{selectedWorktree!.name}</div>
                      <div className="panel-subtitle">vs {selectedBase}</div>
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
                    </div>
                  </div>

                  <Resizer onResize={(dx) => setFilePanelWidth((w) => clamp(w + dx, 180, 480))} />

                  <div className="panel diff-panel">
                    <DiffView file={selectedFile} />
                  </div>
                </>
              )}
            </>
          )}
        </div>
      ) : (
        <div className="focused-layout">
          {wholeAreaEmptyMessage ? (
            <EmptyArea message={wholeAreaEmptyMessage} />
          ) : (
            <>
              <div className="focused-subbar">
                <select className="focused-select" value={projectName} disabled>
                  <option value={projectName}>{projectName}</option>
                </select>
                <select
                  className="focused-select"
                  value={selectedWorktree?.path ?? ""}
                  onChange={(e) => {
                    const wt = worktrees.find((w) => w.path === e.currentTarget.value);
                    if (wt) selectWorktree(wt);
                  }}
                >
                  <option value="" disabled>
                    Select worktree…
                  </option>
                  {worktrees.map((wt) => (
                    <option key={wt.path} value={wt.path}>
                      {wt.name} ({wt.branch ?? "detached"})
                    </option>
                  ))}
                </select>
                {selectedWorktree && (
                  <span className="focused-base">vs {selectedBase}</span>
                )}
                <select
                  className="focused-select"
                  value={selectedFile?.path ?? ""}
                  onChange={(e) => {
                    const f = diff?.files.find((f) => f.path === e.currentTarget.value);
                    if (f) setSelectedFile(f);
                  }}
                  disabled={!diff}
                >
                  <option value="" disabled>
                    Select file…
                  </option>
                  {committedFiles.length > 0 && (
                    <optgroup label="Committed">
                      {committedFiles.map((f) => (
                        <option key={f.path} value={f.path}>
                          {f.path}
                        </option>
                      ))}
                    </optgroup>
                  )}
                  {uncommittedFiles.length > 0 && (
                    <optgroup label="Uncommitted">
                      {uncommittedFiles.map((f) => (
                        <option key={f.path} value={f.path}>
                          {f.path}
                        </option>
                      ))}
                    </optgroup>
                  )}
                </select>
              </div>
              {detailAreaEmptyMessage ? (
                <EmptyArea message={detailAreaEmptyMessage} />
              ) : (
                <div className="panel diff-panel diff-panel--focused">
                  <DiffView file={selectedFile} />
                </div>
              )}
            </>
          )}
        </div>
      )}

      {contextMenu && (
        <>
          <div className="context-menu-overlay" onClick={() => setContextMenu(null)} />
          <div className="context-menu" style={{ left: contextMenu.x, top: contextMenu.y }}>
            <button
              type="button"
              className="context-menu-item context-menu-item--danger"
              onClick={() => {
                removeWorktree(contextMenu.worktree);
                setContextMenu(null);
              }}
            >
              Remove worktree…
            </button>
          </div>
        </>
      )}
    </div>
  );
}

function EmptyArea({ message }: { message: string }) {
  return <div className="empty-area">{message}</div>;
}

function DiffView({ file }: { file: FileDiff | null }) {
  if (!file) return null;
  return (
    <>
      <div className="diff-panel-header">
        <span className="diff-panel-path">{file.path}</span>
        <span className="num diff-add">+{file.additions}</span>
        <span className="num diff-del">-{file.deletions}</span>
      </div>
      <div className="diff-panel-body">
        <SideBySideDiff file={file} />
      </div>
    </>
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
