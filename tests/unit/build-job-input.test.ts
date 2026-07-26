import { describe, expect, it } from "vitest";

import { buildJobInput } from "@/lib/build-job-input";
import { BEST_AUDIO_QUALITY_VALUE } from "@/lib/generic-quality-options";
import type { MediaSource } from "@/types/download";

const AUDIO_PREVIEW: MediaSource = {
  source_url: "https://example.com/v",
  title: "Bài hát",
  thumbnail_url: null,
  duration_seconds: 200,
  platform: "youtube",
  is_playlist: false,
  playlist_item_count: null,
  available_video_qualities: [{ label: "1080p", filesize_bytes: 100 }],
  available_audio_formats: [{ bitrate_kbps: 128, codec: "opus", filesize_bytes: 50 }],
  is_gallery: false,
  gallery_items: [],
  playlist_entries: [],
};

/** Three images plus an audio track — the shape a TikTok slideshow actually
 * has. The image count and the total item count deliberately differ, so a
 * test that says "everything is selected" can only pass if the code compares
 * the selection against the *selectable* (image) items rather than against
 * `gallery_items.length`. */
const GALLERY_PREVIEW: MediaSource = {
  ...AUDIO_PREVIEW,
  is_gallery: true,
  available_video_qualities: [],
  available_audio_formats: [],
  gallery_items: [
    { url: "https://cdn/1.jpg", extension: "jpg", is_audio: false },
    { url: "https://cdn/2.jpg", extension: "jpg", is_audio: false },
    { url: "https://cdn/3.jpg", extension: "jpg", is_audio: false },
    { url: "https://cdn/a.mp3", extension: "mp3", is_audio: true },
  ],
};

describe("buildJobInput", () => {
  it("builds an audio job with the chosen bitrate", () => {
    const input = buildJobInput({
      preview: AUDIO_PREVIEW,
      mediaType: "audio",
      audioQuality: "128kbps",
      videoQuality: null,
      outputDirectory: "/out",
    });

    expect(input).toEqual({
      source_url: "https://example.com/v",
      media_type: "audio",
      audio_quality: "128kbps",
      video_quality: null,
      output_directory: "/out",
      playlist_scope: undefined,
      title: "Bài hát",
    });
  });

  it("sends no audio quality when the source only offered a best-available option", () => {
    const input = buildJobInput({
      preview: AUDIO_PREVIEW,
      mediaType: "audio",
      audioQuality: BEST_AUDIO_QUALITY_VALUE,
      videoQuality: null,
      outputDirectory: "/out",
    });

    // The sentinel is a UI-only radio value; forwarding it would have the
    // backend reject it as a quality the source never advertised.
    expect(input.audio_quality).toBeNull();
  });

  it("builds a video job with the chosen label and no audio quality", () => {
    const input = buildJobInput({
      preview: AUDIO_PREVIEW,
      mediaType: "video",
      audioQuality: "128kbps",
      videoQuality: "1080p",
      outputDirectory: "/out",
    });

    expect(input.media_type).toBe("video");
    expect(input.video_quality).toBe("1080p");
    expect(input.audio_quality).toBeNull();
  });

  it("keeps the chosen video quality out of an audio job", () => {
    const input = buildJobInput({
      preview: AUDIO_PREVIEW,
      mediaType: "audio",
      audioQuality: "128kbps",
      videoQuality: "1080p",
      outputDirectory: "/out",
    });

    expect(input.video_quality).toBeNull();
  });

  it("passes an explicit playlist scope through, defaulting to the single item", () => {
    const playlistPreview: MediaSource = { ...AUDIO_PREVIEW, is_playlist: true };

    expect(
      buildJobInput({
        preview: playlistPreview,
        mediaType: "audio",
        audioQuality: "128kbps",
        videoQuality: null,
        outputDirectory: "/out",
        playlistScope: "entire_playlist",
      }).playlist_scope,
    ).toBe("entire_playlist");

    expect(
      buildJobInput({
        preview: playlistPreview,
        mediaType: "audio",
        audioQuality: "128kbps",
        videoQuality: null,
        outputDirectory: "/out",
      }).playlist_scope,
    ).toBe("single_item");
  });

  it("builds a gallery job carrying the selected indices", () => {
    const input = buildJobInput({
      preview: GALLERY_PREVIEW,
      mediaType: "audio",
      audioQuality: null,
      videoQuality: null,
      outputDirectory: "/out",
      galleryMode: "images_only",
      selectedGalleryIndices: [0, 2],
    });

    expect(input).toEqual({
      source_url: "https://example.com/v",
      media_type: "gallery",
      audio_quality: null,
      video_quality: null,
      output_directory: "/out",
      gallery_mode: "images_only",
      selected_gallery_indices: [0, 2],
      title: "Bài hát",
    });
  });

  it("omits the index list when every image is selected, even though the post also has an audio track", () => {
    const input = buildJobInput({
      preview: GALLERY_PREVIEW,
      mediaType: "audio",
      audioQuality: null,
      videoQuality: null,
      outputDirectory: "/out",
      galleryMode: "files",
      // Every one of the three images — index 3 is the audio track, which the
      // selection grid never offers.
      selectedGalleryIndices: [0, 1, 2],
    });

    // Not the same as sending [0, 1, 2]: the backend re-crawls the post right
    // before downloading, so a full list freezes the item count as it was at
    // preview time.
    expect(input.selected_gallery_indices).toBeUndefined();
    expect("selected_gallery_indices" in input).toBe(false);
  });

  it("omits the index list when no selection was made at all", () => {
    const input = buildJobInput({
      preview: GALLERY_PREVIEW,
      mediaType: "audio",
      audioQuality: null,
      videoQuality: null,
      outputDirectory: "/out",
      galleryMode: "audio_only",
    });

    expect(input.selected_gallery_indices).toBeUndefined();
  });
});
