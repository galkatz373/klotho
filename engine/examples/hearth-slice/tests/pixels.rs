//! PR 12b visual goldens. Structural tests always run; GPU writes BMPs when present.

use std::path::PathBuf;

use hearth_slice::{PixelScene, write_scene_bmp};

#[test]
fn gpu_goldens_write_bmps() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/pixels");
    std::fs::create_dir_all(&dir).unwrap();
    let mut wrote = 0usize;
    for scene in PixelScene::ALL {
        let path = dir.join(format!("{}.bmp", scene.stem()));
        if let Some((w, h, clusters)) = write_scene_bmp(scene, &path) {
            assert!(w >= 320 && h >= 180, "{w}x{h}");
            assert!(clusters >= 8, "{scene:?} clusters {clusters}");
            assert!(path.metadata().unwrap().len() > 54);
            wrote += 1;
        }
    }
    if wrote == 0 {
        eprintln!("no GPU adapter; pixel BMPs skipped");
        return;
    }
    assert_eq!(wrote, 4);
}
