//! Native same-UIView safe-area and committed render-geometry oracle.
use flui::prelude::*;
use flui::types::Size;
use flui::rendering::{BoxConstraints, BoxDryLayoutCtx, BoxLayoutContext, BoxParentData, BoxProtocol, Leaf, PaintCx, RenderBox, RenderUpdateImpact};
use flui::view::{RenderObjectContext, RenderView};
use flui::widgets::{MediaQuery, SafeArea, Stack};
use flui::types::layout::StackFit;
use objc2::MainThreadMarker;
use objc2_ui_kit::{UIApplication, UIWindowScene};
use std::{cell::RefCell, sync::{Mutex, atomic::{AtomicUsize, Ordering}}, time::Duration};
static PAINTS: AtomicUsize = AtomicUsize::new(0);
static AMBIENT: Mutex<[f32;4]> = Mutex::new([0.0;4]);
type Geometry = Vec<(bool, f32, f32, f32, f32)>;
thread_local! { static SNAPSHOT: RefCell<Option<Box<dyn Fn()->Geometry>>> = const { RefCell::new(None) }; }
#[derive(Debug, flui::Diagnosticable)]
struct ProbeLeaf { protected: bool }
impl RenderBox for ProbeLeaf {
    type Arity = Leaf;
    type ParentData = BoxParentData;
    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_,Leaf,BoxParentData>) -> Size { ctx.constraints().biggest() }
    fn compute_dry_layout(&self, constraints: BoxConstraints, _: &mut BoxDryLayoutCtx<'_>) -> Size { constraints.biggest() }
    fn paint(&self, ctx: &mut PaintCx<'_,Leaf>) {
        let size = ctx.size();
        let color = if self.protected { Color::rgb(60,100,180) } else { Color::rgb(180,40,40) };
        ctx.canvas().draw_rect(flui::types::Rect::from_origin_size(flui::types::Point::ZERO,size), &flui::painting::Paint::fill(color));
        PAINTS.fetch_add(1, Ordering::SeqCst);
    }
}
#[derive(Clone)]
struct LeafView(bool);
impl RenderView for LeafView {
    type Protocol = BoxProtocol;
    type RenderObject = ProbeLeaf;
    fn create_render_object(&self, _: &RenderObjectContext<'_>) -> ProbeLeaf { ProbeLeaf { protected: self.0 } }
    fn update_render_object(&self, _: &RenderObjectContext<'_>, _: &mut ProbeLeaf) -> RenderUpdateImpact { RenderUpdateImpact::NONE }
}
flui::view::impl_render_view!(LeafView);
#[derive(Clone, StatefulView)]
struct Root;
struct State;
impl StatefulView for Root { type State=State; fn create_state(&self)->State { State } }
impl ViewState<Root> for State {
    fn init_state(&mut self, ctx:&dyn BuildContext) {
        let pipeline=ctx.pipeline_owner().expect("real pipeline");
        SNAPSHOT.with(|slot| *slot.borrow_mut()=Some(Box::new(move || pipeline.with(|owner| {
            let tree=owner.render_tree();
            tree.iter().filter_map(|(id,node)| {
                let leaf=node.downcast_render_object::<ProbeLeaf>()?;
                let size=node.size()?;
                let (mut x,mut y)=(0.0,0.0);
                let mut next=Some(id);
                while let Some(id)=next { let offset=tree.get(id).expect("node").offset(); x+=offset.dx.get();y+=offset.dy.get();next=tree.parent(id); }
                Some((leaf.protected,x,y,size.width.get(),size.height.get()))
            }).collect()
        }))));
    }
    fn build(&self,_:&Root,ctx:&dyn BuildContext)->impl IntoView {
        let p=MediaQuery::of(ctx).padding;
        *AMBIENT.lock().expect("ambient")=[p.top.get(),p.right.get(),p.bottom.get(),p.left.get()];
        Stack::new((LeafView(false),SafeArea::new().child(LeafView(true)))).fit(StackFit::Expand)
    }
}
#[derive(Clone, StatelessView)]
struct App;
impl StatelessView for App { fn build(&self, _: &dyn BuildContext) -> impl IntoView { Root } }
fn main() {
    std::thread::spawn(|| {
        let deadline=std::time::Instant::now()+Duration::from_secs(20);
        while PAINTS.load(Ordering::SeqCst)<2 {
            if std::time::Instant::now()>deadline { report("FAIL first paint deadline".into());return; }
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_secs(1));
        dispatch2::DispatchQueue::main().exec_async(|| {
            let marker=MainThreadMarker::new().expect("owner");
            let scene=UIApplication::sharedApplication(marker).connectedScenes().into_iter().next().expect("scene");
            let native=scene.downcast_ref::<UIWindowScene>().expect("window scene").windows().into_iter().find(|w|!w.isHidden()).expect("published window");
            let view=native.rootViewController().expect("controller").view().expect("exact root UIView");
            view.layoutIfNeeded();
            let p=view.safeAreaInsets();let bounds=view.bounds().size;
            let expected=[p.top as f32,p.right as f32,p.bottom as f32,p.left as f32];
            let ambient=*AMBIENT.lock().expect("ambient");
            let geometry=SNAPSHOT.with(|slot|slot.borrow().as_ref().expect("snapshot")());
            let protected=geometry.iter().find(|g|g.0).expect("protected geometry");
            let background=geometry.iter().find(|g|!g.0).expect("background geometry");
            let near=|a:f32,b:f32|(a-b).abs()<0.1;
            let passed=expected[0]>0.0 && ambient.iter().zip(expected).all(|(a,b)|near(*a,b))
                && near(protected.1,expected[3]) && near(protected.2,expected[0])
                && near(protected.3,bounds.width as f32-expected[1]-expected[3])
                && near(protected.4,bounds.height as f32-expected[0]-expected[2])
                && near(background.1,0.0)&&near(background.2,0.0)&&near(background.3,bounds.width as f32)&&near(background.4,bounds.height as f32);
            report(format!("{} safe-area native={expected:?} ambient={ambient:?} protected={protected:?} background={background:?}",if passed {"PASS"}else{"FAIL"}));
        });
    });
    run_app(App);
}
fn report(message:String) { std::fs::write(std::env::temp_dir().join("flui-execution-result.txt"),message).expect("marker"); }
