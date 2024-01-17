export interface ChatMessage {
  author: string
  content: string
}

export interface NewMessage {
  chat: string
  author: string
  content: string
}

export interface SendChatMessage {
  Send: {
    target: string
    message: string
  }
}

// Chats consists of a map of counterparty to an array of messages
export interface Chats {
  [counterparty: string]: ChatMessage[]
}
