_Last updated: 2026-11-27 18:00_

# Changing strokes

Pick up any stroke or fill, yours or an agent's, and move it, recolour it, bend it or delete it. klad paints it again in its new state.

![Every stroke of the lantern's lines selected, with the selection bar above them](../images/guide-selection.png)

## Move something

Press **V**, press on what you want to move and drag. Hold **alt** to drag a copy. A shape with no fill is taken by its outline and a text anywhere in its block; a click on a shape inside the one you have selected takes the shape you clicked.

## Select strokes

Press **S** and click a stroke. Shift click adds more. Drag a box around strokes to select them, or pick **loop** or **ellipse** in the bar over the canvas and draw around them. **wand** takes the area of one colour you click, up to its edge; its **tolerance** lets close colours in. Hold **shift** to add to the selection and **alt** to take away from it. The box or loop takes the strokes that are wholly inside it; to take the parts of strokes that cross its edge too, tick **a box or loop cuts strokes at its edge** in the bar. **cmd A** selects everything on the layer at this frame.

To pick on every layer instead of only the active one, switch the bar over the canvas to **all layers**: a click, a loop, the wand and **cmd A** then reach every visible, unlocked layer, and the wand goes to the layer painted under the click.

## Change what you selected

Drag inside the box to move it, a corner to scale it and the knob to turn it. A turned box stays turned, so the next drag works on it as you see it. Hold **shift** to move level or upright, to turn in steps of 15 degrees or, on a corner, to change width and height apart; hold **alt** to scale or stretch from the middle. To type a position, size or angle, click **# numbers** in the bar, type in a field and press Return; the numbers stay open until you click elsewhere. The bar next to the selection recolours it, makes strokes thinner or thicker, duplicates it (**cmd D**) and deletes it (**Backspace**).

To recolour, click the palette button in the bar and pick a colour on the wheel, from the drawing's palette or from the colours you used lately; **use the current colour** gives it the colour you paint with. Recolouring several strokes keeps each one's own lighter or darker shade, so a painted area stays painted.

## Copy and paste

**cmd C** copies the selected strokes or shapes, **cmd X** cuts them, and **cmd V** pastes them in place, on this frame or on another one. Strokes are pasted on the layer you have selected, so to put them on another layer, select that layer before you paste. Shapes pasted while a paint layer is active get a vector layer of their own above it.

Copying also puts a picture of the selection on the clipboard, for other apps. **shift cmd C** copies what you see, every layer together: inside the selection, or the whole canvas. A picture copied in another app pastes with **cmd V** onto a new layer in the middle of the view, selected so you can move it; on a pixel layer it comes in as pixels in the drawing's palette.

## Put something in front or behind

Select it and press **cmd ]** to bring it forward or **cmd [** to send it back; add **shift** to go all the way. **Edit > Arrange** has the same. Strokes go past the next stroke they overlap.

## Split part of a drawing onto its own layer

1. Select the strokes or shapes, such as the roof of a house.
2. Right click the canvas and select **Move to new layer**.

They move to a new layer right above the one they were on and stay selected.

## Move an eraser on its own

1. Select the strokes it erased. The bar shows what is attached to them, such as **1 attached**.
2. Click it, and drag. The eraser attaches to the strokes it lands on.

To let it erase whatever is under it instead, choose **Edit > Strokes > Detach**.

## Bend a stroke

1. Press **A** and click the stroke.
2. Drag a point or a handle. The stroke keeps its pressure, so its taper stays where it was.

Alt click on the stroke adds a point.

## Move a whole layer

Press **cmd T** and drag inside the box to move the layer, a corner to scale it or outside it to turn it. It moves every drawing on the layer; switch off **every drawing** to move only this frame's. Press **Return** when you're done.

## Name the parts of a drawing

Select the strokes of one piece, such as a leaf or a leg, type a name under **part** in layer settings and click **make them this part**. [In-betweens](animating.md#let-klad-draw-the-in-betweens) pair strokes by part.
