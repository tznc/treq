import type { JjCommitsAhead } from "../lib/api";

/** Commits the merge brings into the target, newest first. */
export const MergeCommitsList = ({
  commitsAhead,
  targetBranch,
}: {
  commitsAhead: JjCommitsAhead | null;
  targetBranch: string;
}) => (
  <section>
    <h2 className="text-sm font-semibold mb-3">
      Commits to be merged ({commitsAhead?.total_count || 0})
    </h2>
    {commitsAhead && commitsAhead.commits.length > 0 ? (
      <div className="border rounded-lg divide-y">
        {commitsAhead.commits.map((commit) => (
          <div key={commit.short_id} className="p-3">
            <div className="flex items-start gap-3">
              <code className="text-xs bg-muted px-1.5 py-0.5 rounded font-mono">
                {commit.short_id}
              </code>
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium truncate">
                  {commit.description || "(no description)"}
                </p>
                <div className="flex items-center gap-2 mt-0.5 flex-wrap">
                  <span className="text-xs text-muted-foreground">
                    {commit.author_name}
                  </span>
                  {commit.insertions > 0 || commit.deletions > 0 ? (
                    <span className="text-xs text-muted-foreground">
                      <span className="text-green-600">
                        +{commit.insertions}
                      </span>{" "}
                      <span className="text-red-600">-{commit.deletions}</span>
                    </span>
                  ) : null}
                </div>
              </div>
            </div>
          </div>
        ))}
      </div>
    ) : (
      <div className="border rounded-lg p-6 text-center text-muted-foreground">
        No commits ahead of {targetBranch}
      </div>
    )}
  </section>
);
