_Last updated: 2026-11-21 20:30_

# Working with an agent

Let an agent you already use paint into your drawings: an app such as Claude Desktop or the Codex app, or a coding agent in a terminal. klad has no model inside: the agent works through klad's own commands, and its strokes show up in your editor within about a second.

## Connect Claude Desktop or Codex

No terminal needed.

1. The first time klad opens, click **Get started** in the welcome: its second screen connects your agent app. Later, choose **File > Connect an agent app…**, or **Help > Welcome to klad** to see the welcome again.
2. Each agent app on your computer has a row: Claude Desktop, Codex, Claude Code, Cursor or VS Code. Click **Connect** on yours. klad adds itself to that app's settings and leaves the rest of them alone.
3. Quit the app and open it again, as its row says. klad shows up among its tools, and its row in klad turns from amber to green.
4. Ask in your own words: "Paint a red apple in a new klad drawing and open it in klad." **Copy** next to the example puts it on the clipboard. The app may ask before it uses klad the first time: allow it.

Agents work in your drawings folder and the folders of drawings you opened lately. To let them work somewhere else, open **Where your agent may work** in the same window and click **Add a folder…**.

## In a terminal

1. In the document menu at the foot of the tool strip, click **install the klad command**.
2. Ask your agent to run `klad skill` and read what it prints. For pixel art, ask it to run `klad skill pixel` too.
3. Copy the drawing's path from the File menu and tell the agent what you want: "Paint a lantern on its own layer in art.klad, lit from the left."

## If the app doesn't see klad

- Quit the app completely (cmd Q) and open it again.
- If the app's card says **klad moved since you connected it**, click **Connect again**: klad moved since you connected it, from the disk image to Applications, say.

## Point at something

Press **N** and click the canvas to pin a note there, such as "more moss on this side". The agent reads your notes and what you've selected, and a pin turns green when it's done.

## Undo what the agent did

Press **cmd Z**: the agent's steps are in the same history as yours, and the History pane says who made each one. Its strokes are ordinary strokes, so you can also repaint, move or delete them by hand.
