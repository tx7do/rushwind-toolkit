// Tauri 事件封装，与 gowind-uiapp 的 wailsjs/runtime 同名同签名，
// 页面代码可直接按 Go 版复刻。
import {emit, listen, once} from '@tauri-apps/api/event'

type Listener = (data: any) => void

const unlisteners = new Map<string, Array<() => void>>()

export function EventsOn(event: string, cb: Listener): () => void {
  let cancel: (() => void) | null = null
  let removed = false
  listen(event, (e) => cb(e.payload)).then((un) => {
    if (removed) {
      un()
      return
    }
    cancel = un
    const arr = unlisteners.get(event) ?? []
    arr.push(un)
    unlisteners.set(event, arr)
  })
  return () => {
    removed = true
    cancel?.()
  }
}

export function EventsOnce(event: string, cb: Listener): () => void {
  let cancel: (() => void) | null = null
  once(event, (e) => cb(e.payload)).then((un) => {
    cancel = un
  })
  return () => cancel?.()
}

export function EventsOff(event: string): void {
  const arr = unlisteners.get(event)
  if (arr) {
    arr.forEach((un) => un())
    unlisteners.delete(event)
  }
}

export function EventsEmit(event: string, data?: any): void {
  void emit(event, data ?? null)
}
