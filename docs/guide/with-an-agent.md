_Last updated: 2026-11-20 15:45_

# Working with an agent

Let an agent you already use paint into your drawings: an app such as Claude Desktop or the Codex app, or a coding agent in a terminal. klad has no model inside: the agent works through klad's own commands, and its strokes show up in your editor within about a second.

## Connect Claude Desktop or Codex

No terminal needed.

1. Choose **File > Connect an agent app…**. On the start window, open **Connect an agent app** at the bottom.
2. Click **connect Claude Desktop**, or Codex, Claude Code, Cursor or VS Code. klad adds itself to that app's settings and leaves the rest of them alone.
3. Quit the app and open it again. klad shows up among its tools.
4. Ask in your own words: "Paint a red apple in a new klad drawing and open it in klad." The app may ask before it uses klad the first time: allow it.

Agents work in your drawings folder and the folders of drawings you opened lately. To let them work somewhere else, click **add a folder** in the same window.

## In a terminal

1. In the document menu at the foot of the tool strip, click **install the klad command**.
2. Ask your agent to run `klad skill` and read what it prints. For pixel art, ask it to run `klad skill pixel` too.
3. Copy the drawing's path from the File menu and tell the agent what you want: "Paint a lantern on its own layer in art.klad, lit from the left."

## If the app doesn't see klad

- Quit the app completely (cmd Q) and open it again.
- If the Connect window says **set to a klad that was moved**, click **connect again**: klad moved since you connected it, from the disk image to Applications, say.

## Point at something

Press **N** and click the canvas to pin a note there, such as "more moss on this side". The agent reads your notes and what you've selected, and a pin turns green when it's done.

## Undo what the agent did

Press **cmd Z**: the agent's steps are in the same history as yours, and the History pane says who made each one. Its strokes are ordinary strokes, so you can also repaint, move or delete them by hand.
