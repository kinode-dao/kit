import { create } from 'zustand'
import { NewMessage, Chats } from '../types/Chat'
import { persist, createJSONStorage } from 'zustand/middleware'

export interface ChatStore {
  chats: Chats
  addMessage: (msg: NewMessage) => void
  get: () => ChatStore
  set: (partial: ChatStore | Partial<ChatStore>) => void
}

const useChatStore = create<ChatStore>()(
  persist(
    (set, get) => ({
      chats: { "New Chat": [] },
      addMessage: (msg: NewMessage) => {
        const { chats } = get()
        const { chat, author, content } = msg
        if (!chats[chat]) {
          chats[chat] = []
        }
        chats[chat].push({ author, content })
        set({ chats })
      },

      get,
      set,
    }),
    {
      name: 'chat', // unique name
      storage: createJSONStorage(() => sessionStorage), // (optional) by default, 'localStorage' is used
    }
  )
)

export default useChatStore
