mod support;

use helix_view::document::Mode;
use support::EditorSession;

#[tokio::test]
async fn mode_changes_are_rendered() -> anyhow::Result<()> {
    let mut session = EditorSession::open().await?;
    session.assert_mode(Mode::Normal);
    assert_eq!(session.screen_cell(1, session.status_row()), "N");

    session.type_keys("i").await?;
    session.assert_mode(Mode::Insert);

    session.type_keys("<esc>v").await?;
    session.assert_mode(Mode::Select);

    session.type_keys("<esc>").await?;
    session.assert_mode(Mode::Normal);
    session.close().await
}
