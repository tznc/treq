/**
 * Team projects (077_organization_projects.sql) through PostgREST as signed-in
 * users: what the desktop project sync (core/project_sync.rs) calls. Members
 * put and list their organization's projects; outsiders see and write none;
 * repos must be remote references.
 */
import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
} from "../seed";

it("shares team projects with members only", async () => {
  const admin = getServiceClient();
  const member = await createTestUser();
  const outsider = await createTestUser();
  const orgId = randomUUID();
  const projectId = randomUUID();
  try {
    expect(
      (
        await admin
          .from("organizations")
          .insert({ id: orgId, name: "QA Projects" })
      ).error,
    ).toBeNull();
    expect(
      (
        await admin
          .from("organization_members")
          .insert({ org_id: orgId, user_id: member.user.id, role: "owner" })
      ).error,
    ).toBeNull();

    const { client: m } = await signInWithEmailPassword(
      member.email,
      member.password,
    );
    const put = (repos: unknown, name = "Web") =>
      m.rpc("put_organization_project", {
        p_id: projectId,
        p_org_id: orgId,
        p_name: name,
        p_repos: repos,
        p_prompt: "Use pnpm",
        p_context_paths: [],
      });

    expect((await put(["acme/web"])).error).toBeNull();
    expect(
      (await put(["acme/web", "git@github.com:acme/api.git"], "Web 2")).error,
    ).toBeNull();
    const local = await put(["/Users/me/code/web"]);
    expect(local.error?.code).toBe("23514");

    const listed = await m
      .from("organization_projects")
      .select("id,org_id,name,repos,prompt,context_paths");
    expect(listed.error).toBeNull();
    expect(listed.data).toEqual([
      {
        id: projectId,
        org_id: orgId,
        name: "Web 2",
        repos: ["acme/web", "git@github.com:acme/api.git"],
        prompt: "Use pnpm",
        context_paths: [],
      },
    ]);

    await recordOutcome("organization-projects-01-member-put-and-list", {
      expectations: [
        "A member upserts a team project by id via put_organization_project and lists it back.",
        "A repo given as a local path is rejected with check_violation 23514.",
      ],
      details: { orgId, listed: listed.data, localError: local.error },
    });

    const { client: o } = await signInWithEmailPassword(
      outsider.email,
      outsider.password,
    );
    const seen = await o.from("organization_projects").select("id");
    const write = await o.rpc("put_organization_project", {
      p_id: randomUUID(),
      p_org_id: orgId,
      p_name: "Evil",
      p_repos: [],
      p_prompt: "",
      p_context_paths: [],
    });
    expect(seen.data).toEqual([]);
    expect(write.error?.code).toBe("42501");

    await recordOutcome("organization-projects-02-outsider-denied", {
      expectations: [
        "An outsider lists no team projects.",
        "An outsider's put_organization_project fails with RLS 42501.",
      ],
      details: { seen: seen.data, writeError: write.error },
    });
  } finally {
    await admin.from("organizations").delete().eq("id", orgId);
    await deleteTestUser(admin, member.user.id);
    await deleteTestUser(admin, outsider.user.id);
  }
}, 60000);
