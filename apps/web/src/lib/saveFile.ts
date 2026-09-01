import type { ExportFormat } from '../types/cad'

export interface ExportKind {
  id: ExportFormat
  ext: string
  label: string
  mime: string
  hint: string
  /** Shown when the format is known-broken or experimental — never the default. */
  caution?: string
}

export const EXPORT_KINDS: ExportKind[] = [
  { id: 'stl',  ext: 'stl',  label: 'STL',  mime: 'model/stl',                hint: '3D printing mesh' },
  { id: 'gltf', ext: 'glb',  label: 'GLB',  mime: 'model/gltf-binary',        hint: 'glTF 2.0 binary — realtime 3D' },
  { id: 'obj',  ext: 'obj',  label: 'OBJ',  mime: 'model/obj',                hint: 'Wavefront mesh' },
  { id: 'brep', ext: 'brep', label: 'BREP', mime: 'application/octet-stream', hint: 'Open CASCADE native B-Rep' },
  {
    id: 'step',
    ext: 'step',
    label: 'STEP',
    mime: 'application/step',
    hint: 'CAD interchange — SolidWorks, Fusion, Onshape',
    caution: 'Known broken for the golden M8 bolt — use STL or GLB instead',
  },
]

export function sanitizeExportBase(documentId?: string): string {
  const cleaned = (documentId ?? '').trim().replace(/[^a-zA-Z0-9._-]+/g, '_').replace(/^_+|_+$/g, '')
  return cleaned.slice(0, 80) || 'model'
}

export function exportFileName(documentId: string | undefined, ext: string): string {
  return `${sanitizeExportBase(documentId)}.${ext}`
}

export function canDownloadExport(opts: {
  runError: string | null
  irCode: string
  lastGoodIrCode: string
}): { ok: true } | { ok: false; reason: string } {
  if (opts.runError) {
    return { ok: false, reason: 'Cannot export while a rebuild error is set. Fix or rebuild first.' }
  }
  if (!opts.irCode.trim()) {
    return { ok: false, reason: 'Nothing to export. Generate or paste a CAD program first.' }
  }
  if (!opts.lastGoodIrCode.trim() || opts.irCode !== opts.lastGoodIrCode) {
    return {
      ok: false,
      reason: 'Rebuild the model before exporting. Current IR does not match the last successful run.',
    }
  }
  return { ok: true }
}

interface FilePickerHandle {
  createWritable: () => Promise<{
    write: (data: Blob) => Promise<void>
    close: () => Promise<void>
  }>
}

interface SaveFilePickerWindow {
  showSaveFilePicker: (opts: {
    suggestedName?: string
    types?: Array<{ description: string; accept: Record<string, string[]> }>
  }) => Promise<FilePickerHandle>
}

export type SaveTarget =
  | { kind: 'picker'; handle: FilePickerHandle }
  | { kind: 'download' }
  | { kind: 'cancelled' }

/** Open the OS Save As dialog immediately (must run in a click handler). */
export async function pickSaveTarget(kind: ExportKind, suggestedName?: string): Promise<SaveTarget> {
  const w = window as unknown as SaveFilePickerWindow
  if (typeof w.showSaveFilePicker !== 'function') {
    return { kind: 'download' }
  }
  try {
    const handle = await w.showSaveFilePicker({
      suggestedName: suggestedName ?? exportFileName(undefined, kind.ext),
      types: [
        {
          description: `${kind.label} file`,
          accept: { [kind.mime]: [`.${kind.ext}`] },
        },
      ],
    })
    return { kind: 'picker', handle }
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') {
      return { kind: 'cancelled' }
    }
    return { kind: 'download' }
  }
}

export async function writeSaveTarget(
  target: SaveTarget,
  blob: Blob,
  kind: ExportKind,
  fileName?: string,
): Promise<'saved' | 'cancelled'> {
  if (target.kind === 'cancelled') return 'cancelled'

  if (target.kind === 'picker') {
    const writable = await target.handle.createWritable()
    await writable.write(blob)
    await writable.close()
    return 'saved'
  }

  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = fileName ?? exportFileName(undefined, kind.ext)
  a.click()
  URL.revokeObjectURL(url)
  return 'saved'
}
