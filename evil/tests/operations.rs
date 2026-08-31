mod support;

use helix_view::document::Mode;
use support::EditorSession;

const FIVE_LINES: &str = "one\ntwo\nthree\nfour\nfive\n";

#[tokio::test]
async fn pending_delete_is_rendered_at_the_screen_edge() -> anyhow::Result<()> {
    let mut session = EditorSession::open_with_contents(FIVE_LINES).await?;

    session.type_keys("d").await?;

    let pending_column = session.screen_width() - 15;
    assert_eq!(
        session.screen_cell(pending_column, session.message_row()),
        "d"
    );

    session.type_keys("<esc>").await?;
    session.close().await
}

#[tokio::test]
async fn d3d_deletes_three_lines() -> anyhow::Result<()> {
    let mut session = EditorSession::open_with_contents(FIVE_LINES).await?;

    session.type_keys("d3d").await?;

    session.assert_mode(Mode::Normal);
    assert_eq!(session.text(), "four\nfive\n");
    session.close().await
}

#[tokio::test]
#[ignore = "dG is not implemented as an Evil operator motion yet"]
async fn dg_deletes_until_end_of_file() -> anyhow::Result<()> {
    let mut session = EditorSession::open_with_contents(FIVE_LINES).await?;

    session.type_keys("jdG").await?;

    session.assert_mode(Mode::Normal);
    assert_eq!(session.text(), "one\n");
    session.close().await
}
