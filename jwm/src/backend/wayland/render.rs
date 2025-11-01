// src/backend/wayland/render.rs

use smithay::{
    backend::renderer::{
        damage::{Error as OutputDamageTrackerError, OutputDamageTracker, RenderOutputResult},
        element::{surface::WaylandSurfaceRenderElement, AsRenderElements},
        Frame,
        // FIX: Import ALL necessary traits for smithay-0.7.0 GLES2 renderer
        ImportAll,
        ImportDmaWl,
        ImportMem,
        ImportMemWl,
        Renderer,
    },
    desktop::{
        space::{Space, SpaceRenderElements},
        Window,
    },
    output::Output,
    utils::{Logical, Point},
};

use super::cursor::PointerRenderElement;

// FIX: This definition should now be correct for smithay-0.7.0
// The key is to apply the trait bounds where they are actually required by the inner elements.
smithay::render_elements! {
    pub CustomRenderElements<R> where R: Renderer + ImportAll + ImportMem,
        R::TextureId: Clone + 'static;
    Space=SpaceRenderElements<R, WaylandSurfaceRenderElement<R>>,
    Pointer=PointerRenderElement<R>,
}

#[allow(clippy::too_many_arguments)]
pub fn render_output<'a, R>(
    output: &'a Output,
    space: &'a Space<Window>,
    custom_elements: impl IntoIterator<Item = CustomRenderElements<R>>,
    renderer: &'a mut R,
    damage_tracker: &'a mut OutputDamageTracker,
    age: usize,
) -> Result<RenderOutputResult<'a>, OutputDamageTrackerError<R::Error>>
where
    // FIX: Add all required bounds for the Renderer generic R
    R: Renderer + Frame<Error = R::Error, Texture = R::TextureId> + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    let space_elements =
        smithay::desktop::space::space_render_elements(renderer, [space], output, 1.0f32)
            .expect("Failed to render space");

    let mut render_elements: Vec<CustomRenderElements<R>> = space_elements
        .into_iter()
        .map(CustomRenderElements::from)
        .collect();

    render_elements.extend(custom_elements);

    damage_tracker.render_output(renderer, age, &render_elements, [0.1, 0.1, 0.1, 1.0])
}
