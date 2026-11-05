//! Vector footage layer binding and undo tests.

use serde_json::json;

use crate::Session;

#[test]
fn relink_cropped_vector_preserves_binding_and_undo() {
    use effectcraft_project::{Footage, FootageKind, ItemKind, SourceLayer};
    struct VectorImporter;
    impl crate::Importer for VectorImporter {
        fn probe(&self, path: &str) -> Result<Footage, String> {
            Ok(Footage { path: path.into(), kind: FootageKind::Still, codec: "AI".into(), width: 1000, height: 800, has_video: true, ..Default::default() })
        }
    }
    let mut session = Session { importer: Some(std::sync::Arc::new(VectorImporter)), ..Default::default() };
    let binding = SourceLayer { index: 2, name: "Hand".into(), layer_size: true, crop_bounds: Some([20.25, 20.5, 40.5, 60.75]), ..Default::default() };
    let item = std::sync::Arc::make_mut(&mut session.project).add_item(
        "Hand",
        Default::default(),
        None,
        ItemKind::Footage(Footage {
            path: "/offline/art.ai".into(),
            kind: FootageKind::Still,
            width: 21,
            height: 41,
            missing: true,
            layer: Some(binding.clone()),
            ..Default::default()
        }),
    );
    session.state.project_selection = vec![item];
    let before = session.project.to_file_json().unwrap();
    session.execute("file.replaceFootage", json!({"item":item.0,"path":"/relinked/art.ai"})).unwrap();
    let reopened = effectcraft_project::Project::from_json(&session.project.to_file_json().unwrap()).unwrap();
    let ItemKind::Footage(footage) = &reopened.item(item).unwrap().kind else { panic!("not footage") };
    assert_eq!(footage.layer.as_ref(), Some(&binding));
    assert_eq!((footage.width, footage.height), (21, 41));
    assert_eq!(footage.path, "/relinked/art.ai");
    assert!(!footage.missing);
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(session.project.to_file_json().unwrap(), before);
}
