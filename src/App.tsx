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

type FileDiff = {
  path: string;
  status: string;
  additions: number;
  deletions: number;
  section: "committed" | "uncommitted";
  patch: string;
};

type DiffResult = {
  merge_base_oid: string;
  head_oid: string;
  files: FileDiff[];
};

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
          <div style={{ flex: 1 }}>
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
                <pre style={{ overflowX: "auto", background: "#111", color: "#eee", padding: "0.5rem" }}>
                  {f.patch}
                </pre>
              </details>
            ))}
          </div>
        )}
      </div>
    </main>
  );
}

export default App;
