_Last updated: 2026-11-19 23:45_

# Painting

Paint with klad's brushes, blend colours on the canvas and make brushes of your own.

![Painting the lemons with the oil brush](../images/guide-painting.png)

## Paint and erase

Press **B** and paint on a paint layer. **E** switches to the eraser and back, and so does the eraser end of the pen. **[** and **]** make the brush smaller and bigger. With the eraser on, **feather** in the bar fades its edge like an airbrush.

A click paints a dot, and a stroke ends where you lift the pen. For strokes that start and end in a point, click **more** in the brush bar and set **taper in px** and **taper out px**.

An eraser stroke stays with the strokes it erased, like a mask that travels with them: move them and the cut goes along, without biting into what they land on. Smudges and blends stay with their strokes the same way. [Move an eraser on its own](selecting.md#move-an-eraser-on-its-own) to change that.

## Paint a straight line

Hold **shift** and click: klad paints a straight line from the end of your last stroke. Hold **shift** while you drag to keep the line level, upright or at 45 degrees.

For a line, a rectangle or an ellipse in paint, click **more** in the brush bar, pick it next to **draws** (the menu stays open while you set it up) and drag. **shift** makes a square or a circle, **alt** draws from the middle. While it is on, the bar shows it; pick **free** to paint by hand again.

## Pick a brush and a colour

Click a brush in the **Brush** pane: Inking pen, Pencil and Marker for lines, Oil, Loaded oil and the bristle brushes for paint. Lean the pen with the Pencil to shade: the line grows wider and lighter. Hold **alt** and click the canvas to pick up a colour from it.

In the **Colour** pane, mix a colour on the wheel or with the sliders next to it. **HSV**, **HSL**, **RGB** and **Lab** above the sliders choose what they change: for the shadow of a colour, pick **Lab** and lower **L**.

## Blend colours

Paint with **Smudge**, **Wet round** or **Blender** over colours that are already there. They only mix paint on the layer they paint on, so put the colours you want to blend on one layer.

## Make paint look thick

1. Paint with **Oil** or **Loaded oil**.
2. In layer settings, pick **relief** from **add an effect**.
3. Drag on the light dial towards where the light comes from.

## Change the canvas

**Edit > Canvas** holds what changes the whole drawing; each is one step that **cmd Z** takes back.

- **Canvas size…** opens a window: type the new size, click where the drawing should stay and click **change the canvas size**. The same window scales the whole drawing and sets the paper colour.
- **Crop** (**C**, the crop tool in the strip): a box with handles goes round the canvas. Drag a handle to move its edge (**shift** keeps the shape, **alt** sizes from the middle), drag inside to move the box, or drag a new one; past the canvas edges it grows the canvas, as Canvas size does. The bar shows its size. Press **Return** or **crop to the box**; **Esc** leaves. **Crop to selection** and **Trim** crop to the selection or to what is drawn.
- **Turn** and **Flip** turn or mirror every layer at once.

## Paint symmetrically

Turn on **mirror** in the brush bar. Every stroke is painted with its mirror image across an upright line.

## Paint in perspective

1. Choose **View > Drawing guides…** and click **2-point perspective** (or one or three points, a **ruler** or an **ellipse**).
2. Drag the guide's points (the white dots) on the canvas while the window is open, or later with **V**: the vanishing points, the ruler's ends, the ellipse's middle and sides.
3. Paint. Each stroke runs to the vanishing point it heads for, or stays upright or level. Near a ruler a stroke runs on it, further away parallel to it; an ellipse takes strokes round it.

Hold **cmd** while you paint to draw freely, or turn off **follow** in the brush bar.

## Make your own brush

![The brush editor over the canvas](../images/guide-brush.png)

1. Press **F5** to open the brush editor.
2. Change the tip, the pressure or the kind of paint, and watch the sample stroke at the top.
3. Type a name and click **save in this drawing**, or **save in your brushes** to have it in every drawing.

Right click a brush of the drawing in the brush tiles to save it in **your brushes**. Your brushes are listed under the others in every drawing; right click one to rename, group, move or forget it. A drawing that uses one keeps its own copy, so it looks the same on another computer. The buttons next to **your brushes** write them all to one file to share, and add a shared file's brushes to yours.

## Tune your pen

1. Open the settings with **cmd ,** and go to **Pen**.
2. Press on the tablet as hard as you ever draw, then click **heaviest = my peak**.
3. If light strokes still come out too heavy, set the curve above 1.
