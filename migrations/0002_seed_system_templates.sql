INSERT INTO extraction_templates (name, description, json_schema, prompt_template, is_system)
VALUES (
    'Invoice',
    'Extract structured data from invoices',
    '{
        "type": "object",
        "properties": {
            "vendor_name": {"type": "string"},
            "invoice_number": {"type": "string"},
            "date": {"type": "string"},
            "due_date": {"type": "string"},
            "line_items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "description": {"type": "string"},
                        "quantity": {"type": "number"},
                        "unit_price": {"type": "number"},
                        "total": {"type": "number"}
                    }
                }
            },
            "subtotal": {"type": "number"},
            "tax": {"type": "number"},
            "total": {"type": "number"},
            "currency": {"type": "string"},
            "payment_terms": {"type": "string"}
        }
    }'::jsonb,
    'Extract all invoice data from this document. Return structured data matching the schema. Extract line items with descriptions, quantities, unit prices and totals. Identify the vendor, invoice number, dates, and payment information.',
    true
),
(
    'Receipt',
    'Extract structured data from receipts',
    '{
        "type": "object",
        "properties": {
            "merchant_name": {"type": "string"},
            "date": {"type": "string"},
            "time": {"type": "string"},
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "quantity": {"type": "number"},
                        "price": {"type": "number"}
                    }
                }
            },
            "subtotal": {"type": "number"},
            "tax": {"type": "number"},
            "tip": {"type": "number"},
            "total": {"type": "number"},
            "payment_method": {"type": "string"},
            "last_four_digits": {"type": "string"}
        }
    }'::jsonb,
    'Extract all receipt data from this document. Return structured data matching the schema. Identify each purchased item with its name, quantity and price. Extract the merchant name, date, time, totals, and payment method details.',
    true
);
