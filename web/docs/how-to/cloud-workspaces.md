---
sidebar_position: 4
---

# Cloud Workspaces

_Create a Treq-managed cloud workspace, open or clone repositories on it, and know what happens when it suspends or your subscription lapses._

:::note[Work in progress]

Managed cloud workspaces ship in v0.3.0. The latest published release is v0.2.0, so the Cloud workspace card is not in it yet.

:::

A **cloud workspace** is a persistent virtual machine that Treq creates and maintains for your account. Treq, JJ, Git, and coding agents come preinstalled. Location, CPU, memory, and storage are managed for you. In this version each account has one cloud workspace, and you keep multiple repositories on it.

Your Treq window stays on your machine. Terminals, agents, and repository files live on the cloud workspace, the same remote-control model as [Remote SSH Workspaces](/docs/how-to/remote-ssh-workspaces). You do not need the Remote SSH feature preview to use a cloud workspace. That preview only covers machines you run yourself.

## Requirements

- A Treq account you are signed in to.
- A **Pro** or **Team** subscription, listed on the [pricing](/pricing) page.

On the Free plan, **Create cloud workspace** returns a **PRO** badge, the server's message, and an **Upgrade to Pro** button. The button opens your dashboard on the Treq website.

## Create the workspace

1. Open Settings and choose **Account**.
2. In the **Cloud workspace** card, select **Create cloud workspace**.
3. Wait while the card reports each stage: **Creating cloud workspace...**, **Installing Treq, JJ, Git, and agents...**, **Configuring access...**, and **Verifying readiness...**.
4. When the card shows **Ready**, the workspace accepts connections.

You can also start from **Open cloud workspace** on the start screen or **File → Open via SSH...**. Both open the cloud workspace setup directly. With the Remote SSH preview turned on, the start screen button reads **Open via SSH** and the **Connect a remote repository** dialog lets you choose **Treq-managed cloud workspace** or **Your own VM**.

If creation fails before a machine exists, the button reads **Retry cloud workspace creation**.

## Open or clone a repository

1. On a **Ready** card, select **Open repositories**.
2. Under **Saved repositories**, select a repository you opened before. Otherwise type its location in **Remote repository path** and select **Probe**.
3. If the path holds a repository, select **Open**.
4. If it does not, enter a **Git URL to clone** and select **Clone repository**.
5. To start an empty repository instead, check **Initialize an empty repository at this path** and select **Initialize repository**.

Repeat these steps for each repository. They all live on the one cloud workspace.

## Suspend and wake

An idle cloud workspace suspends, and the card shows **Suspended (idle)**. Disk contents are kept. While a repository is open, a banner above it reads **Waking managed VM...** with a **Wake now** link. On the card, select **Wake** to do the same. The card shows **Waking...** and returns to **Ready** when the machine is back.

The card also shows disk use against your quota and the number of workspaces and repositories on the machine.

## Repair or delete

When the card shows **Degraded** or **Failed**, a **Repair** button appears. It reruns Treq setup on the machine and keeps your files and repositories.

The trash icon (**Delete cloud workspace**) removes the machine. The confirmation dialog warns that cloned repositories, uncommitted changes, and agent sessions are lost, and that you cannot undo it. Deleting your Treq account deletes the cloud workspace and its disk too.

## If your subscription lapses

Creating, waking, and repairing a cloud workspace need an active Pro or Team subscription, and so does connecting to it. When the subscription ends, the machine stops. Treq keeps the disk for 7 days and then deletes it.

Resubscribe within those 7 days to wake the workspace with your files. After that the data is gone, and you create a new cloud workspace. Checking status and deleting the workspace still work without a subscription.

## Next Steps

- [Remote SSH Workspaces](/docs/how-to/remote-ssh-workspaces) for machines you run yourself
- [Customizing Settings](/docs/how-to/customizing-settings)
