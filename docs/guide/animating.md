_Last updated: 2026-11-20 00:35_

# Animating frame by frame

Draw an animation one drawing at a time, check it against the drawing before, and play it. klad can draw the in-betweens for you.

![The creature's walk in the timeline, with onion skin on](../images/guide-animation.png)

## Draw the next frame

1. Draw the first pose.
2. Press **shift K**. You get a blank drawing on the next frame.
3. Press **O** for onion skin: the drawing before shows in red under the one you're drawing.
4. Draw the next pose, and repeat.
5. Press **Return** to play, and again to stop.

A drawing of one frame shows the timeline folded to its bar. It opens when the second drawing is made, or with the arrow at its left.

To animate on twos, click **2s** next to **shift K on** in the timeline header first.

## Flip through your drawings

Press **,** and **.** to go a frame back and forward. Or hold **Q** and move the pen sideways to flip through the drawings like the corner of a stack of paper.

## Change the timing

Click the drawing's cell in the timeline, then press **=** to hold it a frame longer and **-** to take that back. Later drawings move along.

These keys, and **I** to insert a frame, work while the timeline is in use: a blue line around the frames shows that it has the keys. After a stroke on the canvas, click a frame first, or use the **Frame** menu, which works from anywhere.

## Start a pose from the last one

Right click the drawing's cell in the timeline and pick **duplicate to the next frame**, then change the copy into the next pose.

## Repeat a run of frames

1. Drag over the frames in the timeline. To take every frame, click a frame and press **cmd A**.
2. Press **cmd D**, or click the duplicate button above the frames.

The frames are repeated right after themselves as copies, and the copies are selected, so **cmd D** again adds another round. While frames are selected, **cmd C** and **cmd V** copy and paste them, and **Delete** removes their drawings.

## Make clips for a game

Drag along the empty strip above the frame numbers to make a clip, then click its bar to name it (idle, walk, attack). Each clip [exports](exporting.md) as its own animation.

## Bring in frames from another app

1. Select **File > Import frames…**.
2. Click **Choose…** for a sprite sheet, or **Folder…** for a folder of numbered pictures (`run-0001.png`, `run-0002.png` …).
3. For a sheet, cut it **by cell size** or **by columns and rows**, and set the space **between** and **around** the cells. The preview shows the cut and how many frames it makes; empty cells are left out.
4. Click **Import**.

The frames go on a new layer, one per frame, with a clip named after the file. In a pixel drawing their colours take the palette.

## Let klad draw the in-betweens

1. Draw two keys with empty frames between them, and [name the parts](selecting.md#name-the-parts-of-a-drawing) of both.
2. Click the first key's cell and shift click the second one's.
3. Click **in-between** in the selection bar.
4. Choose how many drawings to make and how they're spaced, and click **make the in-betweens**.

After you change a key, select the range again and click **redo the in-betweens in the selection**.
