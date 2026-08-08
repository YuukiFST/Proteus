//! One view per tool, PRD §9/§10: the app shell (left rail + active view) and
//! the per-tool view modules. Views call `proteus-core` for every operation —
//! this crate holds UI glue only (PRD §6/§7 T0 tier).

pub mod common;
pub mod html_to_pdf;
pub mod image_compress;
pub mod image_convert;
pub mod image_crop;
pub mod image_resize;
pub mod image_rotate;
pub mod image_watermark;
pub mod pdf_compress;
pub mod pdf_crop;
pub mod pdf_merge;
pub mod pdf_organize;
pub mod pdf_page_numbers;
pub mod pdf_protect;
pub mod pdf_rotate;
pub mod pdf_split;
pub mod pdf_to_a;
pub mod pdf_to_jpg;
pub mod pdf_unlock;
pub mod pdf_watermark;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::*;
use gpui_component::*;

/// Every PRD §9 tool, one entry per view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    PdfMerge,
    PdfSplit,
    PdfCompress,
    PdfOrganize,
    PdfRotate,
    PdfToJpg,
    PageNumbers,
    CropMargins,
    ProtectPdf,
    UnlockPdf,
    PdfWatermark,
    HtmlToPdf,
    PdfToPdfA,
    ImageCompress,
    ImageResize,
    ImageCrop,
    ImageConvert,
    ImageWatermark,
    ImageRotate,
}

impl Tool {
    pub const ALL: [Tool; 19] = [
        Tool::PdfMerge,
        Tool::PdfSplit,
        Tool::PdfCompress,
        Tool::PdfOrganize,
        Tool::PdfRotate,
        Tool::PdfToJpg,
        Tool::PageNumbers,
        Tool::CropMargins,
        Tool::ProtectPdf,
        Tool::UnlockPdf,
        Tool::PdfWatermark,
        Tool::HtmlToPdf,
        Tool::PdfToPdfA,
        Tool::ImageCompress,
        Tool::ImageResize,
        Tool::ImageCrop,
        Tool::ImageConvert,
        Tool::ImageWatermark,
        Tool::ImageRotate,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tool::PdfMerge => "Merge",
            Tool::PdfSplit => "Split",
            Tool::PdfCompress => "Compress PDF",
            Tool::PdfOrganize => "Organize",
            Tool::PdfRotate => "Rotate PDF",
            Tool::PdfToJpg => "PDF → JPG",
            Tool::PageNumbers => "Page numbers",
            Tool::CropMargins => "Crop margins",
            Tool::ProtectPdf => "Protect",
            Tool::UnlockPdf => "Unlock",
            Tool::PdfWatermark => "Watermark PDF",
            Tool::HtmlToPdf => "HTML → PDF",
            Tool::PdfToPdfA => "PDF/A",
            Tool::ImageCompress => "Compress",
            Tool::ImageResize => "Resize",
            Tool::ImageCrop => "Crop",
            Tool::ImageConvert => "Convert",
            Tool::ImageWatermark => "Watermark",
            Tool::ImageRotate => "Rotate",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            Tool::PdfMerge
            | Tool::PdfSplit
            | Tool::PdfCompress
            | Tool::PdfOrganize
            | Tool::PdfRotate
            | Tool::PdfToJpg
            | Tool::PageNumbers
            | Tool::CropMargins
            | Tool::ProtectPdf
            | Tool::UnlockPdf
            | Tool::PdfWatermark
            | Tool::HtmlToPdf
            | Tool::PdfToPdfA => "PDF tools",
            _ => "Image tools",
        }
    }
}

/// The `AnyView` per tool is created once at startup and kept alive so switch
/// between tools preserves each tool's in-progress state.
pub struct AppView {
    active: Tool,
    views: Vec<(Tool, AnyView)>,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let views = vec![
            (Tool::PdfMerge, cx.new(|cx| pdf_merge::MergePdfView::new(window, cx)).into()),
            (Tool::PdfSplit, cx.new(|cx| pdf_split::SplitPdfView::new(window, cx)).into()),
            (Tool::PdfCompress, cx.new(|cx| pdf_compress::CompressPdfView::new(window, cx)).into()),
            (Tool::PdfOrganize, cx.new(|cx| pdf_organize::OrganizePdfView::new(window, cx)).into()),
            (Tool::PdfRotate, cx.new(|cx| pdf_rotate::RotatePdfView::new(window, cx)).into()),
            (Tool::PdfToJpg, cx.new(|cx| pdf_to_jpg::PdfToJpgView::new(window, cx)).into()),
            (Tool::PageNumbers, cx.new(|cx| pdf_page_numbers::PageNumbersView::new(window, cx)).into()),
            (Tool::CropMargins, cx.new(|cx| pdf_crop::CropMarginsView::new(window, cx)).into()),
            (Tool::ProtectPdf, cx.new(|cx| pdf_protect::ProtectPdfView::new(window, cx)).into()),
            (Tool::UnlockPdf, cx.new(|cx| pdf_unlock::UnlockPdfView::new(window, cx)).into()),
            (Tool::PdfWatermark, cx.new(|cx| pdf_watermark::PdfWatermarkView::new(window, cx)).into()),
            (Tool::HtmlToPdf, cx.new(|cx| html_to_pdf::HtmlToPdfView::new(window, cx)).into()),
            (Tool::PdfToPdfA, cx.new(|cx| pdf_to_a::PdfToAView::new(window, cx)).into()),
            (Tool::ImageCompress, cx.new(|cx| image_compress::ImageCompressView::new(window, cx)).into()),
            (Tool::ImageResize, cx.new(|cx| image_resize::ImageResizeView::new(window, cx)).into()),
            (Tool::ImageCrop, cx.new(|cx| image_crop::ImageCropView::new(window, cx)).into()),
            (Tool::ImageConvert, cx.new(|cx| image_convert::ImageConvertView::new(window, cx)).into()),
            (Tool::ImageWatermark, cx.new(|cx| image_watermark::ImageWatermarkView::new(window, cx)).into()),
            (Tool::ImageRotate, cx.new(|cx| image_rotate::ImageRotateView::new(window, cx)).into()),
        ];
        Self {
            active: Tool::PdfMerge,
            views,
        }
    }

    fn sidebar_items(&self) -> impl Iterator<Item = (Tool, &str)> {
        Tool::ALL.iter().map(|&t| (t, t.group()))
    }
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active;
        let current = self
            .views
            .iter()
            .find(|(tool, _)| *tool == active)
            .map(|(_, view)| view.clone())
            .expect("all tools must have a view");

        // Sidebar: group headers + one nav button per tool.
        let mut nav_children: Vec<AnyElement> = Vec::new();
        let mut last_group: Option<&str> = None;
        for (tool, group) in self.sidebar_items() {
            if last_group != Some(group) {
                nav_children.push(
                    div()
                        .mt_3()
                        .mb_1()
                        .text_xs()
                        .text_color(gpui_component::gray(400))
                        .child(group.to_string())
                        .into_any_element(),
                );
                last_group = Some(group);
            }
            let this = cx.entity();
            nav_children.push(
                Button::new(tool.title())
                    .label(tool.title())
                    .ghost()
                    .selected(active == tool)
                    .on_click(move |_, _, cx| {
                        this.update(cx, |s: &mut AppView, cx| {
                            s.active = tool;
                            cx.notify();
                        })
                    })
                    .into_any_element(),
            );
        }
        let nav = v_flex().gap_1().w(px(200.)).p_4().children(nav_children);

        h_flex()
            .size_full()
            .child(
                div()
                    .flex_shrink_0()
                    .h_full()
                    .border_r_1()
                    .border_color(gpui_component::gray(200))
                    .bg(gpui_component::gray(50))
                    .child(nav),
            )
            .child(div().flex_1().size_full().child(current))
    }
}