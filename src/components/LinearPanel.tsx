import { ChevronDown, Zap } from "lucide-react";
import { useState } from "react";
import useSWR from "swr";
import { linearListTeams } from "../lib/api-linear";
import type { LinearIssueAttachment } from "../lib/promptAttachments";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { Tabs, TabsList, TabsTrigger } from "./ui/tabs";
import { LinearIssuesSection } from "./LinearIssuesPanel";
import { LinearProjectsSection } from "./LinearProjectsPanel";
import { LinearReviewContext } from "./LinearAgentReview";
import type { ReviewLaunch } from "../lib/agent-review-launch";

interface LinearPanelProps {
  repoPath: string;
  onStartPromptFromIssue?: (issue: LinearIssueAttachment) => void;
  /** Starts an agent session that reviews an issue, project or document. */
  onStartAgentReview?: (launch: ReviewLaunch) => Promise<void>;
}

type MainSection = "issues" | "projects";

export const LinearPanel: React.FC<LinearPanelProps> = ({
  repoPath,
  onStartPromptFromIssue,
  onStartAgentReview,
}) => {
  const [section, setSection] = useState<MainSection>("issues");
  const [selectedTeam, setSelectedTeam] = useState<string | undefined>(
    undefined,
  );

  const { data: teams = [] } = useSWR(
    repoPath ? ["linear-teams", repoPath] : null,
    async () => await linearListTeams(repoPath),
    { revalidateOnFocus: false },
  );

  return (
    <LinearReviewContext.Provider value={{ repoPath, onStartAgentReview }}>
      <div
        className="flex h-full bg-background flex-col"
        data-testid="linear-panel"
      >
        <div className="flex items-center justify-between px-4 pt-4 pb-2 shrink-0">
          <div className="flex items-center gap-2">
            <Zap className="w-5 h-5 text-muted-foreground" />
            <h1 className="text-base font-semibold leading-tight">Linear</h1>
          </div>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="gap-2 text-sm"
                data-testid="linear-team-selector"
              >
                {selectedTeam
                  ? teams.find((t) => t.key === selectedTeam)?.name ||
                    selectedTeam
                  : "All Teams"}
                <ChevronDown className="w-4 h-4 text-muted-foreground" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuLabel>Teams</DropdownMenuLabel>
              <DropdownMenuSeparator />
              <DropdownMenuRadioGroup value={selectedTeam || ""}>
                <DropdownMenuRadioItem
                  value=""
                  onSelect={() => setSelectedTeam(undefined)}
                >
                  All Teams
                </DropdownMenuRadioItem>
                {teams.map((team) => (
                  <DropdownMenuRadioItem
                    key={team.key}
                    value={team.key}
                    onSelect={() => setSelectedTeam(team.key)}
                  >
                    {team.name}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        <div className="px-4 pb-2 shrink-0">
          <Tabs
            value={section}
            onValueChange={(v) => setSection(v as MainSection)}
          >
            <TabsList className="text-base">
              <TabsTrigger value="issues">Issues</TabsTrigger>
              <TabsTrigger value="projects">Projects</TabsTrigger>
            </TabsList>
          </Tabs>
        </div>

        {section === "issues" ? (
          <LinearIssuesSection
            repoPath={repoPath}
            selectedTeam={selectedTeam}
            onStartPromptFromIssue={onStartPromptFromIssue}
          />
        ) : (
          <LinearProjectsSection repoPath={repoPath} />
        )}
      </div>
    </LinearReviewContext.Provider>
  );
};
