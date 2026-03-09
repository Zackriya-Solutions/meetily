"""Shared test helper functions."""


def _register_payload(email="test@example.com", password="TestPass1", device_id="device-001"):
    return {"email": email, "password": password, "device_id": device_id}


def _login_payload(email="test@example.com", password="TestPass1", device_id="device-001"):
    return {"email": email, "password": password, "device_id": device_id}
