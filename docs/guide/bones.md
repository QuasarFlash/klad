_Last updated: 2026-11-20 01:50_

# Posing with bones

Draw a character once, give it a skeleton and pose it on each frame instead of redrawing it. The skeleton goes on a bones layer of its own, and you tell each drawing layer which bone moves it.

Bones are a preview: posing may still change in later versions, and your drawings keep their rigs when it does.

![The rig sample's skeleton in the bone tool, with the forearm bone picked](../images/guide-bones.png)

## Draw a skeleton

1. Click **Layer > New bones layer**.
2. Press **J** and pick **draw** in the bar over the canvas.
3. Drag from the hips to draw the first bone, click to add the next one, and press **Esc** to end the chain.

Each bone hangs from the one before it, and turning a bone turns everything that hangs from it, so start in the middle of the body and work outwards. Alt drag from a bone's end to start a branch there, such as an arm off the spine.

## Attach the drawing to the bones

Draw each part that stays stiff, like an upper arm, a forearm or the head, on its own paint layer. The layers can sit anywhere in the layers panel.

1. In **draw**, click a bone to pick it.
2. Open **layers: 0** and tick the layers that move with that bone.

## Bend a layer across a joint

Optional, for a part drawn in one piece over a joint, like a sleeve or a tail.

1. In the layers panel, click the layer's bone button and pick **bends with bones**.
2. Pick **weights** in the bar, then **⋯ > automatic**.
3. Cmd click a bone, then paint where it should move the drawing.

The weight brush has four modes in the bar: **paint** (toward the weight, **shift 1**), **add** (**shift 2**), **remove** (**shift 3**) and **smooth** (**shift 4**). Alt swaps add and remove while you paint. **weight** is the value a dab paints toward, adds or takes away. Under **⋯** in the bar, **strength** is how far one dab goes, so 0.1 builds a soft joint up over a few strokes and 1 sets it at once; the pen's pressure scales it. **⋯** also has the brush's **hardness**.

The number by the pointer is the picked bone's weight there, as the drawing uses it after every bone's share. **alt I** picks it up as the brush's weight. The overlay's colours run from none to all of it, as the legend under **⋯** shows; **show every bone's weight** there colours each bone in its own colour.

## Pose it

1. Go to a frame and pick **pose**.
2. Drag a bone's end to turn it. The drawing turns with it as you drag.
3. Click **make this pose a drawing** to keep the pose on this frame.

Nothing is kept until you click it. Poses in-between on the bones ([Animating](animating.md)), so a limb swings around its joint instead of sliding.

## Let a leg reach

1. In **draw**, pick the shin, open **add IK** and click **make it a limb with thigh**.
2. In **pose**, drag the shin's end. Both bones turn so the foot lands where you drag.

## If a part doesn't move

- **Turning a bone moves nothing:** the layer isn't rigged. Its bone button in the layers panel is grey; attach it to a bone.
- **Turning the head turns the whole body:** the chain started at the head, so everything hangs from it. In **draw**, pick the head bone and set its **parent** to the neck or the body.
