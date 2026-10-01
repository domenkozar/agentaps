use super::*;
use crate::images::{read_image_file, supported_mime_type};
use gpui_kit::{ClipboardEntry, ClipboardItem, ExternalPaths};

type ImageBytes = Result<(String, Vec<u8>), String>;

/// Larger text files are sent as links so a prompt stays a reasonable size.
const MAX_EMBEDDED_FILE_BYTES: u64 = 512 * 1024;

fn read_file(path: &Path) -> Result<ChatFile, String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| format!("Could not attach {}", path.display()))?;
    let error = |error: std::io::Error| format!("Could not attach {name}: {error}");
    let uri = url::Url::from_file_path(path)
        .map_err(|()| format!("Could not attach {name}: the path is not absolute"))?
        .to_string();
    let metadata = std::fs::metadata(path).map_err(error)?;
    if !metadata.is_file() {
        return Err(format!("Could not attach {name}: it is not a file"));
    }
    let text = if metadata.len() <= MAX_EMBEDDED_FILE_BYTES {
        String::from_utf8(std::fs::read(path).map_err(error)?).ok()
    } else {
        None
    };
    Ok(ChatFile { name, uri, text })
}

fn image_files(paths: &ExternalPaths) -> Vec<ImageBytes> {
    paths
        .paths()
        .iter()
        .filter_map(|path| read_image_file(path))
        .collect()
}

fn clipboard_images(item: &ClipboardItem) -> Vec<ImageBytes> {
    item.entries()
        .iter()
        .flat_map(|entry| match entry {
            ClipboardEntry::Image(image) if supported_mime_type(image.format.mime_type()) => {
                vec![Ok((
                    image.format.mime_type().to_owned(),
                    image.bytes.clone(),
                ))]
            }
            ClipboardEntry::ExternalPaths(paths) => image_files(paths),
            ClipboardEntry::Image(_) | ClipboardEntry::String(_) => Vec::new(),
        })
        .collect()
}

impl Workspace {
    pub(super) fn paste_images(&mut self, item: &ClipboardItem, cx: &mut Context<Self>) -> bool {
        let images = clipboard_images(item);
        if images.is_empty() {
            return false;
        }
        self.attach_images(images, cx);
        true
    }

    pub(super) fn drop_paths(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        self.attach_paths(paths.paths(), cx);
    }

    pub(super) fn choose_attachments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.conversation.file_dialog_open {
            return;
        }
        self.conversation.file_dialog_open = true;
        let pane_id = self.pane_id;
        let session_id = self.view.displayed_session().map(|location| {
            self.projects[location.project_index].agents[location.agent_index]
                .config
                .id
        });
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = selection.await;
            let _ = this.update(cx, |this, cx| {
                this.with_pane(pane_id, cx, |this, cx| {
                    this.conversation.file_dialog_open = false;
                    let current_session = this.view.displayed_session().map(|location| {
                        this.projects[location.project_index].agents[location.agent_index]
                            .config
                            .id
                    });
                    if current_session != session_id {
                        return;
                    }
                    match result {
                        Ok(Ok(Some(paths))) => this.attach_paths(&paths, cx),
                        Ok(Ok(None)) => {}
                        Ok(Err(error)) => {
                            this.notice = Some(Notice::Error(format!(
                                "Could not open file chooser: {error}"
                            )));
                            cx.notify();
                        }
                        Err(error) => {
                            this.notice = Some(Notice::Error(format!(
                                "File chooser closed unexpectedly: {error}"
                            )));
                            cx.notify();
                        }
                    }
                });
            });
        })
        .detach();
    }

    fn attach_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let mut images = Vec::new();
        let mut files = Vec::new();
        for path in paths {
            match read_image_file(path) {
                Some(image) => images.push(image),
                None => files.push(read_file(path)),
            }
        }
        if !images.is_empty() {
            self.attach_images(images, cx);
        }
        if !files.is_empty() {
            self.attach_files(files, cx);
        }
    }

    fn attach_files(&mut self, files: Vec<Result<ChatFile, String>>, cx: &mut Context<Self>) {
        let Some(SessionLocation {
            project_index,
            agent_index,
        }) = self.view.displayed_session()
        else {
            return;
        };
        let agent = &self.projects[project_index].agents[agent_index];
        if agent.protocol.is_none() {
            self.notice = Some(Notice::Error(
                "Wait for the agent to connect before adding files.".into(),
            ));
        } else {
            let agent_id = agent.config.id;
            for file in files {
                match file {
                    Ok(file) => self.draft_files.entry(agent_id).or_default().push(file),
                    Err(error) => self.notice = Some(Notice::Error(error)),
                }
            }
        }
        cx.notify();
    }

    pub(super) fn remove_draft_file(
        &mut self,
        agent_id: u64,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(files) = self.draft_files.get_mut(&agent_id)
            && index < files.len()
        {
            files.remove(index);
            cx.notify();
        }
    }

    pub(super) fn remove_draft_image(
        &mut self,
        agent_id: u64,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(images) = self.draft_images.get_mut(&agent_id)
            && index < images.len()
        {
            images.remove(index);
            cx.notify();
        }
    }

    fn attach_images(&mut self, images: Vec<ImageBytes>, cx: &mut Context<Self>) {
        let Some(SessionLocation {
            project_index,
            agent_index,
        }) = self.view.displayed_session()
        else {
            return;
        };
        let agent = &self.projects[project_index].agents[agent_index];
        if agent.protocol.is_none() {
            self.notice = Some(Notice::Error(
                "Wait for the agent to connect before adding images.".into(),
            ));
        } else if !agent.accepts_images {
            self.notice = Some(Notice::Error("This agent does not accept images.".into()));
        } else {
            let agent_id = agent.config.id;
            for image in images {
                match image.and_then(|(mime_type, bytes)| self.images.save(&mime_type, &bytes)) {
                    Ok(image) => self.draft_images.entry(agent_id).or_default().push(image),
                    Err(error) => self.notice = Some(Notice::Error(error)),
                }
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Image, ImageFormat};

    #[test]
    fn files_keep_utf8_contents_and_link_everything_else() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes.md");
        std::fs::write(&notes, "# Notes").unwrap();
        let binary = temp.path().join("data.bin");
        std::fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
        let large = temp.path().join("large.log");
        std::fs::write(&large, "a".repeat(MAX_EMBEDDED_FILE_BYTES as usize + 1)).unwrap();

        let file = read_file(&notes).unwrap();
        assert_eq!(file.name, "notes.md");
        assert_eq!(file.text.as_deref(), Some("# Notes"));
        assert_eq!(
            url::Url::parse(&file.uri).unwrap().to_file_path().unwrap(),
            notes
        );
        assert_eq!(read_file(&binary).unwrap().text, None);
        assert_eq!(read_file(&large).unwrap().text, None);
        assert!(read_file(temp.path()).is_err());
        assert!(read_file(&temp.path().join("missing.txt")).is_err());
    }

    #[test]
    fn clipboard_images_come_from_image_data_and_copied_image_files() {
        let temp = tempfile::tempdir().unwrap();
        let photo = temp.path().join("photo.jpg");
        std::fs::write(&photo, b"jpeg").unwrap();
        let notes = temp.path().join("notes.txt");
        std::fs::write(&notes, b"text").unwrap();
        let item = ClipboardItem::from(ClipboardEntry::Image(Image::from_bytes(
            ImageFormat::Png,
            b"png".to_vec(),
        )));
        assert_eq!(
            clipboard_images(&item),
            [Ok(("image/png".to_owned(), b"png".to_vec()))]
        );
        let item = ClipboardItem::from(ClipboardEntry::ExternalPaths(ExternalPaths(
            [photo, notes].into_iter().collect(),
        )));
        assert_eq!(
            clipboard_images(&item),
            [Ok(("image/jpeg".to_owned(), b"jpeg".to_vec()))]
        );
        assert!(clipboard_images(&ClipboardItem::new_string("text".into())).is_empty());
        let item = ClipboardItem::from(ClipboardEntry::Image(Image::from_bytes(
            ImageFormat::Bmp,
            b"bmp".to_vec(),
        )));
        assert!(clipboard_images(&item).is_empty());
    }
}
