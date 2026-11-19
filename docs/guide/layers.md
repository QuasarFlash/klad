_Last updated: 2026-11-19 22:45_

# Layers, masks and clipping

Keep lines, colour and shading on their own layers, and decide where each one shows.

## Add a layer

At the top of the Layers pane, click **paint** for brush strokes or **vector** for [shapes and text](shapes-and-text.md). Drag a row to move the layer. Double click its name and type to rename it.

Right click a layer's row for its menu: rename, duplicate, delete, merge, group, lock, mask and more. **alt ]** and **alt [** go to the layer above or below; **alt cmd ]** and **alt cmd [** move the layer up or down. Cmd click more rows to select several layers: hide, lock, group, duplicate and delete then act on all of them.

To put part of a drawing on a layer of its own, [select it](selecting.md#split-part-of-a-drawing-onto-its-own-layer), right click the canvas and select **Move to new layer**.

## Merge layers

1. Select the upper layer.
2. Choose **Merge down** from its right-click menu or from **Layer > Merge**.

Strokes, pixels and shapes stay editable. Layers merge with their own kind, and klad says why when two can't merge. **Merge selected** merges the layers you cmd clicked; **Merge visible** merges every visible layer, into one picture when they differ. **Ungroup** takes a group apart.

## Shade inside flat colour

1. Put the flat colour on one layer.
2. Add a paint layer above it and tick **clip to layer below** in layer settings.
3. Set the new layer's blend mode to **multiply** in the Layers pane's title row and paint the shadows. They stay inside the colour.

## Recolour line art

1. Select the lines layer and tick **lock alpha** in layer settings.
2. Paint over the lines with a big soft brush. Only the lines take the colour.
3. Untick **lock alpha** when you're done, or your next strokes on that layer won't show.

## Hide part of a layer

1. In layer settings, click **add mask**.
2. Paint where the layer should be hidden, and erase to bring it back. Hidden parts show red.
3. Click **back to the layer** to paint on the layer itself again.

## See one layer alone

Alt click its eye in the Layers pane. Alt click it again to bring the others back.

## Trace over a picture

Drop a PNG or JPEG on the window and tick **reference** in its layer settings. (A `.klad` drawing dropped on the window opens instead.) You keep seeing it while you draw, and exports leave it out.

To start a drawing from a picture, choose it in **File > Open**: klad makes a drawing next to it, of its size, with the picture on a layer (a picture of 512 px or less a side comes in as a pixel drawing). **File > New from clipboard** does the same with a picture copied in another app.
