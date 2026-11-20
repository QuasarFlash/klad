_Last updated: 2026-11-20 00:10_

# Exporting

Export a picture, a PDF, a video, numbered frames or a sprite sheet, or send your animation straight into a Godot project.

## Export a picture or frames

1. Select **File > Export** and pick what to write: **Image…** (**cmd E**) for the frame you're on, **Frames…** for one PNG per frame, **Sprite sheet…**, **GIF…** for an animated picture, **PDF…**, **Video…** or **Aseprite file…**.
2. To say where it goes, click **Choose…** next to **folder** and type a name under **save as**. Left empty, the file goes next to the drawing, named after it. The line above **Export** shows where it will go.
3. Click **Export**.

A message says where the file is, and **Show the folder** in the window opens that folder.

Each window shows only what that export uses. Pictures are transparent unless you tick **keep the background colour**. A sprite sheet comes with a JSON file in Aseprite's layout, with a tag per clip.

## Pick a picture format

Under **format** in the Image window:

- **PNG** keeps every pixel and the transparency. Use it unless you have a reason not to.
- **JPEG** makes small files for photos and the web. It has no transparency: white shows there, or the canvas colour with **keep the background colour**. Set the **quality**.
- **WebP** makes small files with transparency for the web. Tick **lossless** to keep every pixel.
- **TIFF** is for print and photo apps, in 8 or 16 bits.

The GIF window has the same choice between **GIF** and **WebP**: an animated WebP keeps full colour and soft edges.

## Export the selection alone

Select strokes with **S** or shapes with **V**, then open **File > Export > Image…**. **only the selection** is ticked: the picture holds what you selected, cut to it. Untick it for the whole image.

## Export a PDF

1. Select **File > Export > PDF…**.
2. Under **pages**, pick **this frame**, **every frame**, **a page per clip** or one clip.
3. Under **paper**, keep **the drawing's size** and set its **pixels per inch**, or pick a paper such as **A4** and tick **landscape** if you need it. On a paper the picture is fitted with a margin.
4. Click **Export**.

Vector layers stay shapes in the PDF, sharp at any size. Paint and pixel layers go in as pictures.

## Export a video

1. Select **File > Export > Video…**.
2. Under **frames**, type **all**, a clip's name or a range such as `0-23`, and set the **size** and **quality**.
3. Click **Export**.

The video is an MP4 that plays at the timeline's speed, with each frame held as long as in klad. It is AV1, which plays in browsers, in VLC and on newer Macs and phones. Transparency becomes white, or the canvas colour with **keep the background colour**.

## Export an SVG

1. Select **File > Export > SVG…**.
2. Leave **paint and pixel layers as pictures** on to keep everything, or turn it off for the shapes alone. Tick **text as outlines** when the SVG goes where the font is missing.
3. Click **Export**.

Each layer becomes a group with its name. With shapes selected, the SVG holds only them. To bring an SVG in, select it in **File > Import picture**, drop it on the window, or open it with **File > Open** as a drawing of its own.

## Export palette variants

If the drawing has [palette variants](pixel-art.md), select **every variant** under **palette variant** before you click **Export**. You get one file for each, with the variant's name in the file name.

## Export into a Godot project

1. Select **File > Export > Godot…**.
2. Next to **Godot folder**, click **Choose…** and pick a folder in your Godot project.
3. Pick what to write: **an animated sprite**, **a parallax scene**, **a rig** or **a tile set**.
4. Click **Export**.

An animated sprite gives you the sheet, a SpriteFrames resource with one animation per clip and a scene to drop in. klad remembers the folder, and exporting again keeps Godot's ids, so scenes that use the files keep working. To export the sprite again without the window, select **File > Export > Godot sprite, same folder**.

## Export from a script

```sh
klad export art.klad --scale 2
klad export art.klad --godot ../game/art/hero
```

Every option is in the [command reference](../cli.md#klad-export).
