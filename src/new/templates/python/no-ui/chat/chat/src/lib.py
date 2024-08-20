import json

import chat_template_dot_os_v0
from chat_template_dot_os_v0.imports.standard import (
    Address,
    MessageRequest,
    MessageResponse,
    ProcessId,
    Request,
    Response,
    print_to_terminal,
    receive,
    send_and_await_response,
    send_response,
)
from chat_template_dot_os_v0.types import Err

def parse_address(address_string):
    node, _, rest = address_string.partition("@")
    process, _, rest = rest.partition(":")
    package, _, rest = rest.partition(":")
    publisher, _, rest = rest.partition(":")

    return node, process, package, publisher

def add_to_archive(conversation, author, content, message_archive):
    message = {
        "author": author,
        "content": content,
    }
    if conversation in message_archive:
        message_archive[conversation].append(message)
    else:
        message_archive[conversation] = [message]
    return message_archive

def handle_message(our_node, message_archive):
    result = receive()
    if isinstance(result, Err):
        raise Exception(f"got error: {result}")
    source, message = result

    match message:
        case MessageResponse():
            raise Exception(f"unexpected Response: {message}")
        case MessageRequest():
            body = json.loads(message.value.body.decode("utf-8"))
            if "Send" in body:
                target, message_text = body["Send"]["target"], body["Send"]["message"]
                if target == our_node:
                    print_to_terminal(0, f"{source.node}: {message_text}")
                    message_archive = add_to_archive(
                        source.node,
                        source.node,
                        message_text,
                        message_archive,
                    )
                else:
                    send_and_await_response(
                        Address(
                            target,
                            ProcessId("chat", "chat", "template.os"),
                        ),
                        Request(False, 5, message.value.body, None, []),
                        None,
                    )
                    message_archive = add_to_archive(
                        target,
                        our_node,
                        message_text,
                        message_archive,
                    )
                send_response(
                    Response(False, json.dumps({"Send": None}).encode("utf-8"), None, []),
                    None,
                )
            elif "History" in body:
                node = body["History"]
                send_response(
                    Response(
                        False,
                        json.dumps({"History": message_archive.get(node, [])}).encode("utf-8"),
                        None,
                        [],
                    ),
                    None,
                )
            else:
                raise Exception(f"Unexpected Request: {body}")

    return message_archive

class ChatTemplateDotOsV0(chat_template_dot_os_v0.ChatTemplateDotOsV0):
    def init(self, our):
        print_to_terminal(0, "chat: begin (python)")

        our_node, _, _, _ = parse_address(our)
        message_archive = {}

        while True:
            try:
                message_archive = handle_message(our_node, message_archive)
            except Exception as e:
                print_to_terminal(0, f"chat: error: {e}")
