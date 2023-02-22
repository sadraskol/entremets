Unique Constraint
==================

Starting from version :code:`0.1.0-alpha.3`,
:code:`entremets` supports unique constraint.
Unique constraint are useful to avoid having duplicate data.

For now :code:`entremets` only support unicity on a single column.

You can declare unique constraint like you would in sql:

.. code-block:: entremets

    init do
      `create unique index on users(id)`
      `create unique index on users(external_id)`
    end

