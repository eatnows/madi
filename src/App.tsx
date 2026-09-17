import { useState } from "react";
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
  const bg =
    line?.tag === "delete" ? "#3a1d1d" : line?.tag === "insert" ? "#1d3a22" : "transparent";
  return (
    <>
      <span className="lineno">{lineno ?? ""}</span>
      <span className="code" style={{ background: bg }}>
        {line?.segments.map((s, i) => (
          <span
            key={i}
            style={
              s.emphasized
                ? {
                    background: line.tag === "delete" ? "#6e2b2b" : "#2b6e3a",
                    borderRadius: 2,
                  }
                : undefined
            }
          >
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
            ⋯
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

function App() {
  const [repoPath, setRepoPath] = useState("");
  const [baseBranch, setBaseBranch] = useState("main");
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [selected, setSelected] = useState<WorktreeInfo | null>(null);
  const [diff, setDiff] = useState<DiffResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function loadWorktrees() {
    setError(null);
    setDiff(null);
    setSelected(null);
    try {
      setWorktrees(await invoke<WorktreeInfo[]>("list_worktrees", { repoPath }));
    } catch (e) {
      setWorktrees([]);
      setError(String(e));
    }
  }

  async function selectWorktree(wt: WorktreeInfo) {
    setSelected(wt);
    setDiff(null);
    setError(null);
    try {
      setDiff(
        await invoke<DiffResult>("diff_against_base", {
          worktreePath: wt.path,
          baseBranch,
        }),
      );
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <main className="container">
      <h1>Worktrees</h1>

      <form
        className="row"
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
          style={{ maxWidth: 120 }}
        />
        <button type="submit">Scan</button>
      </form>

      {error && <p style={{ color: "red" }}>{error}</p>}

      <div style={{ display: "flex", gap: "2rem", textAlign: "left" }}>
        <ul style={{ minWidth: 260 }}>
          {worktrees.map((wt) => (
            <li key={wt.path}>
              <button onClick={() => selectWorktree(wt)}>
                <strong>{wt.name}</strong>
                {wt.is_main && " (main)"} — {wt.branch ?? "(detached)"}
              </button>
            </li>
          ))}
        </ul>

        {selected && diff && (
          <div style={{ flex: 1, minWidth: 0 }}>
            <h2>
              {selected.name} vs {baseBranch}
            </h2>
            <p>
              merge-base {diff.merge_base_oid.slice(0, 7)} .. HEAD {diff.head_oid.slice(0, 7)}
            </p>
            {diff.files.length === 0 && <p>No differences.</p>}
            {diff.files.map((f) => (
              <details key={`${f.section}-${f.path}`} open>
                <summary>
                  [{f.section}] {f.status} {f.path} (+{f.additions} -{f.deletions})
                </summary>
                <SideBySideDiff file={f} />
              </details>
            ))}
          </div>
        )}
      </div>
    </main>
  );
}

export default App;
